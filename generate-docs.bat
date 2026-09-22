@echo off
setlocal

if defined VSCMD_VER goto build

set "vswhere=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%vswhere%" (
    echo Visual Studio Installer could not be found. 1>&2
    exit /b 1
)

for /f "usebackq tokens=*" %%i in (`"%vswhere%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "vs_install=%%i"
if not defined vs_install (
    echo Visual Studio with C++ tools could not be found. 1>&2
    exit /b 1
)

call "%vs_install%\Common7\Tools\VsDevCmd.bat" -arch=x64 || exit /b 1

:build
pushd "%~dp0" || exit /b 1
cmake --preset windows-release -DKIYOSI_BUILD_DOCS=ON || goto error
cmake --build --preset windows-release --target kiyosi-docs || goto error
popd
exit /b 0

:error
popd
exit /b 1
