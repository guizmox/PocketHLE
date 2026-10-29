@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0setup-android-signing.ps1"
set "result=%errorlevel%"
echo.
echo Press any key to close this window...
pause >nul
exit /b %result%
