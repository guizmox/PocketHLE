@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0backup-android-adb.ps1" %*
set "result=%errorlevel%"
if not "%result%"=="0" echo Backup failed. KEEP THE APP INSTALLED.
echo.
echo Press any key to close this window...
pause >nul
exit /b %result%
