@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0google-auth.ps1" %*
