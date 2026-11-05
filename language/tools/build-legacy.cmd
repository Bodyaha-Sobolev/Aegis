@echo off
setlocal
pushd "%~dp0.."
if not exist tools\fasm.exe (
    echo Place a local FASM executable at tools\fasm.exe to build the archived prototype.
    goto failed
)
tools\fasm.exe legacy\asm\src\aegis.asm legacy\asm\aegis.exe
if errorlevel 1 goto failed
tools\fasm.exe -dAEGIS_CONSOLE=1 legacy\asm\src\aegis.asm legacy\asm\aegisc.exe
if errorlevel 1 goto failed
popd
exit /b 0
:failed
popd
exit /b 1
