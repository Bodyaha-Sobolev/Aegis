@echo off
setlocal
pushd "%~dp0"
tools\fasm.exe src\aegis.asm aegis.exe
if errorlevel 1 goto failed
tools\fasm.exe -dAEGIS_CONSOLE=1 src\aegis.asm aegisc.exe
if errorlevel 1 goto failed
if /I "%~1"=="test" (
    py tools\test.py
    if errorlevel 1 goto failed
)
popd
exit /b 0
:failed
popd
exit /b 1
