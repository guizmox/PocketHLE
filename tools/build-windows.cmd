@echo off
setlocal
rem Native Windows build, using the same MSVC/LLVM setup as the user's GDI tree.
set "VSDEVCMD=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if not exist "%VSDEVCMD%" (
  echo Visual Studio 2022 C++ Build Tools are required.
  exit /b 1
)
call "%VSDEVCMD%" >nul
if errorlevel 1 exit /b %errorlevel%
if exist D:\.cargo\bin\cargo.exe set "PATH=D:\.cargo\bin;%PATH%"
rem CMake/MSVC precompiled headers need an ASCII registry path on this host.
rem Respect an explicitly selected Cargo home.
if not defined CARGO_HOME if exist D:\.cargo\registry set "CARGO_HOME=D:\.cargo"
set "VSLANG=1033"
set "PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;%PATH%"
if not defined LIBCLANG_PATH if exist "D:\Program Files\LLVM\bin\libclang.dll" set "LIBCLANG_PATH=D:\Program Files\LLVM\bin"
cd /d "%~dp0.."
if "%~1"=="" (
  cargo build --release -p pocket-cli --features unicorn
) else (
  cargo %*
)
exit /b %errorlevel%
