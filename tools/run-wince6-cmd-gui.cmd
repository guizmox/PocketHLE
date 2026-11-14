@echo off
setlocal
set "POCKETHLE_CE_GUI=%~dp0..\target\release\pockethle-gui.exe"
if not "%~1"=="" set "POCKETHLE_CE_GUI=%~1"
if not exist "%POCKETHLE_CE_GUI%" set "POCKETHLE_CE_GUI=%~dp0..\target\debug\pockethle-gui.exe"
if not exist "%POCKETHLE_CE_GUI%" (
    echo Build pockethle-gui first or pass its path as the first argument.
    exit /b 1
)
"%POCKETHLE_CE_GUI%" --ce-profile "%~dp0..\profiles\wince6-cmd.json" --windowed
