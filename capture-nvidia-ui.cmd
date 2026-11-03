@echo off
setlocal
set "POCKETHLE_UI_TRACE=1"
set "RUST_LOG=info"
if exist "%~dp0target\release\pockethle-gui.exe" (
  "%~dp0target\release\pockethle-gui.exe"
) else (
  echo Build first: cargo build --release -p pocket-desktop
  pause
)
endlocal
