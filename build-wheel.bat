@echo off
setlocal EnableDelayedExpansion

set "vswhere=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%vswhere%" (
    echo Visual Studio installation not found: "%vswhere%" 1>&2
    exit /b 1
)

for /f "usebackq delims=" %%I in (`"%vswhere%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "vs_path=%%I"
if not defined vs_path (
    echo Visual Studio C++ build tools not found. 1>&2
    exit /b 1
)

call "!vs_path!\Common7\Tools\VsDevCmd.bat" -arch=x64
if errorlevel 1 exit /b %ERRORLEVEL%

pushd "%~dp0"
uv build
set "exit_code=%ERRORLEVEL%"
popd

exit /b %exit_code%
