@echo off
rem Shim Windows: o daily-tui chama `mstodo`; roda o script Python via uv (PEP 723).
uv run --script "%~dp0mstodo" %*
