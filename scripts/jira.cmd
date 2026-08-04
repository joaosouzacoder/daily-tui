@echo off
rem Shim Windows: o daily-tui chama `jira`; roda o script Python via uv (PEP 723).
uv run --script "%~dp0jira" %*
