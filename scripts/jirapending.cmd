@echo off
rem Shim Windows: o daily-tui chama `jirapending`; roda a porta PowerShell nativa.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0jirapending.ps1" %*
