@echo off
setlocal

if not "%~1"=="" set "VCPKG_ROOT=%~1"
if not defined VCPKG_ROOT (
    echo Set VCPKG_ROOT or pass the vcpkg directory as the first argument. 1>&2
    exit /b 1
)
set "selected_vcpkg=%VCPKG_ROOT%"
if not exist "%VCPKG_ROOT%\scripts\buildsystems\vcpkg.cmake" (
    echo vcpkg toolchain not found under "%VCPKG_ROOT%". 1>&2
    exit /b 1
)
if not exist "%VCPKG_ROOT%\installed\x64-windows-static\share\QuantLib\QuantLibConfig.cmake" (
    echo QuantLib:x64-windows-static is not installed under "%VCPKG_ROOT%". 1>&2
    exit /b 1
)

if /I "%VSCMD_ARG_TGT_ARCH%"=="x64" goto build

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
set "VCPKG_ROOT=%selected_vcpkg%"
pushd "%~dp0" || exit /b 1
set "build_dir=build\windows-benchmarks"
cmake --fresh -S . -B "%build_dir%" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded "-DCMAKE_TOOLCHAIN_FILE=%selected_vcpkg%\scripts\buildsystems\vcpkg.cmake" -DVCPKG_TARGET_TRIPLET=x64-windows-static -DKIYOSI_BUILD_BENCHMARKS=ON -DBUILD_TESTING=OFF -DKIYOSI_BUILD_EXAMPLES=OFF || goto error
cmake --build "%build_dir%" --target kiyosi_benchmarks || goto error
"%build_dir%\benchmarks\kiyosi_benchmarks.exe" --benchmark_repetitions=5 --benchmark_report_aggregates_only=true "--benchmark_out=%build_dir%\benchmarks.json" --benchmark_out_format=json || goto error
echo Results: %CD%\%build_dir%\benchmarks.json
popd
exit /b 0

:error
set "status=%errorlevel%"
popd
exit /b %status%
