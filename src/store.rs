//! Persistência local em SQLite: notificações já lidas e cache de pastas.
//!
//! Duas coisas precisam sobreviver ao fechar o programa. As notificações que
//! você já resolveu — senão a central volta cheia das mesmas menções a cada
//! abertura. E a lista de pastas do e-mail: buscá-la no IMAP leva segundos, e
//! o seletor de "mover" não pode esperar por isso na primeira vez.
//!
//! Só a thread da UI fala com o banco: as escritas nascem de teclas e o cache
//! de pastas é gravado quando a lista chega no `update`. Nada aqui roda no
//! worker, então não há conexão compartilhada entre threads.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::data::Account;

/// Banco aberto.
pub struct Store {
    conn: Connection,
}

/// Onde o banco vive: no diretório de dados do usuário, ao lado do que as
/// outras ferramentas guardam.
pub fn default_path() -> PathBuf {
    // `cfg!` em vez de `#[cfg]`: assim o compilador checa os dois ramos em
    // qualquer plataforma (ver a nota em `config::default_path`).
    let base = if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    } else {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|_| PathBuf::from("."))
    };
    base.join("daily-tui").join("daily-tui.db")
}

impl Store {
    /// Abre (criando, se preciso) o banco no caminho padrão.
    pub fn open() -> Result<Self, String> {
        Self::open_at(&default_path())
    }

    /// Abre um banco num caminho dado. Criar o diretório é parte do trabalho:
    /// na primeira execução ele não existe.
    pub fn open_at(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("não deu para criar {}: {e}", dir.display()))?;
        }
        let conn = Connection::open(path)
            .map_err(|e| format!("não deu para abrir {}: {e}", path.display()))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Cria o esquema. `IF NOT EXISTS` em tudo: roda em toda abertura.
    fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS notifications_read (
                     id TEXT PRIMARY KEY,
                     at TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS folders (
                     account  TEXT    NOT NULL,
                     position INTEGER NOT NULL,
                     name     TEXT    NOT NULL,
                     at       TEXT    NOT NULL,
                     PRIMARY KEY (account, position)
                 );",
            )
            .map_err(|e| format!("esquema do banco: {e}"))
    }

    /// Notificações já lidas. Vira filtro da central na abertura do programa.
    pub fn read_notifications(&self) -> Result<HashSet<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM notifications_read")
            .map_err(|e| format!("lendo notificações lidas: {e}"))?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("lendo notificações lidas: {e}"))?
            .collect::<Result<HashSet<String>, _>>()
            .map_err(|e| format!("lendo notificações lidas: {e}"))?;
        Ok(ids)
    }

    /// Marca uma notificação como lida. Repetir é inofensivo (`OR REPLACE`).
    pub fn mark_notification_read(&self, id: &str, at: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO notifications_read (id, at) VALUES (?1, ?2)",
                (id, at),
            )
            .map(|_| ())
            .map_err(|e| format!("marcando notificação como lida: {e}"))
    }

    /// Grava as pastas de uma conta, substituindo o que estava lá.
    ///
    /// A ordem importa — o seletor mostra as canônicas primeiro —, então ela é
    /// gravada como coluna em vez de depender da ordem de leitura.
    pub fn save_folders(&self, account: Account, names: &[String], at: &str) -> Result<(), String> {
        let key = account.slot_key();
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("gravando pastas: {e}"))?;
        tx.execute("DELETE FROM folders WHERE account = ?1", [key])
            .map_err(|e| format!("gravando pastas: {e}"))?;
        {
            let mut stmt = tx
                .prepare("INSERT INTO folders (account, position, name, at) VALUES (?1, ?2, ?3, ?4)")
                .map_err(|e| format!("gravando pastas: {e}"))?;
            for (i, name) in names.iter().enumerate() {
                stmt.execute((key, i as i64, name, at))
                    .map_err(|e| format!("gravando pastas: {e}"))?;
            }
        }
        tx.commit().map_err(|e| format!("gravando pastas: {e}"))
    }

    /// Pastas em cache, por conta, na ordem em que foram gravadas.
    ///
    /// Chave desconhecida (banco de uma versão antiga) é ignorada em vez de
    /// virar erro: o cache é conveniência, não fonte da verdade.
    pub fn folders(&self) -> Result<HashMap<Account, Vec<String>>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT account, name FROM folders ORDER BY account, position")
            .map_err(|e| format!("lendo pastas: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("lendo pastas: {e}"))?;

        let mut out: HashMap<Account, Vec<String>> = HashMap::new();
        for row in rows {
            let (account, name) = row.map_err(|e| format!("lendo pastas: {e}"))?;
            if let Some(account) = Account::from_slot_key(&account) {
                out.entry(account).or_default().push(name);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Banco novo num arquivo só deste teste — o nome carrega o do teste para
    /// duas execuções em paralelo não se atropelarem.
    fn temp_store(name: &str) -> (Store, PathBuf) {
        let path = std::env::temp_dir()
            .join("daily-tui-tests")
            .join(format!("{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        (Store::open_at(&path).expect("abre o banco"), path)
    }

    #[test]
    fn the_default_path_lands_in_the_user_data_directory() {
        let path = default_path();
        assert_eq!(path.file_name().unwrap(), "daily-tui.db");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "daily-tui");
        assert!(path.is_absolute() || path.starts_with("."));
    }

    #[test]
    fn a_read_notification_stays_read_after_reopening() {
        // É o ponto da persistência: fechar o programa não desfaz a leitura.
        let (store, path) = temp_store("notifications");
        store
            .mark_notification_read("jira:ENG-101", "2026-08-04T10:00:00-03:00")
            .unwrap();
        drop(store);

        let store = Store::open_at(&path).unwrap();
        let read = store.read_notifications().unwrap();
        assert!(read.contains("jira:ENG-101"));
        assert_eq!(read.len(), 1);
    }

    #[test]
    fn marking_the_same_notification_twice_is_not_an_error() {
        let (store, _) = temp_store("notifications-twice");
        let at = "2026-08-04T10:00:00-03:00";
        store.mark_notification_read("jira:ENG-101", at).unwrap();
        store.mark_notification_read("jira:ENG-101", at).unwrap();
        assert_eq!(store.read_notifications().unwrap().len(), 1);
    }

    #[test]
    fn folders_come_back_in_the_order_they_were_saved_per_account() {
        let (store, path) = temp_store("folders");
        let at = "2026-08-04T10:00:00-03:00";
        let work = vec!["INBOX".to_string(), "Clientes".to_string()];
        let personal = vec!["INBOX".to_string(), "Faturas".to_string()];
        store.save_folders(Account::Work, &work, at).unwrap();
        store.save_folders(Account::Personal, &personal, at).unwrap();
        drop(store);

        let store = Store::open_at(&path).unwrap();
        let cached = store.folders().unwrap();
        assert_eq!(cached.get(&Account::Work), Some(&work));
        assert_eq!(cached.get(&Account::Personal), Some(&personal));
    }

    #[test]
    fn saving_folders_again_replaces_the_previous_list() {
        // Etiqueta apagada no Gmail não pode sobreviver no cache.
        let (store, _) = temp_store("folders-replace");
        let at = "2026-08-04T10:00:00-03:00";
        store
            .save_folders(
                Account::Work,
                &["INBOX".to_string(), "Antiga".to_string()],
                at,
            )
            .unwrap();
        store
            .save_folders(Account::Work, &["INBOX".to_string()], at)
            .unwrap();
        assert_eq!(
            store.folders().unwrap().get(&Account::Work),
            Some(&vec!["INBOX".to_string()])
        );
    }

    #[test]
    fn an_unknown_account_in_the_cache_is_ignored() {
        // Cache de uma versão que tinha outra conta não pode virar erro.
        let (store, _) = temp_store("folders-unknown");
        store
            .conn
            .execute(
                "INSERT INTO folders (account, position, name, at) VALUES ('antiga', 0, 'X', 'ontem')",
                [],
            )
            .unwrap();
        assert!(store.folders().unwrap().is_empty());
    }
}
