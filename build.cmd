@echo off
setlocal
pushd "%~dp0"
set "AEGIS_PYTHON=py"
where py >nul 2>nul
if errorlevel 1 set "AEGIS_PYTHON=python"
if /I "%~1"=="test" (
    %AEGIS_PYTHON% tools\verify_vendor.py
    if errorlevel 1 goto failed
    cargo fmt --package aegis-lang --package aegis-cli --package aegis-studio -- --check
    if errorlevel 1 goto failed
    cargo clippy --locked --workspace --all-targets -- -D warnings
    if errorlevel 1 goto failed
    cargo test --locked --workspace
    if errorlevel 1 goto failed
    cargo build --locked -p aegis-cli
    if errorlevel 1 goto failed
    %AEGIS_PYTHON% tools\test_native.py
    if errorlevel 1 goto failed
    %AEGIS_PYTHON% tools\test_windows_abi.py
    if errorlevel 1 goto failed
    popd
    exit /b 0
)
cargo build --locked --release --workspace
if errorlevel 1 goto failed
copy /Y target\release\aegis-studio.exe aegis.exe >nul
if errorlevel 1 goto failed
copy /Y target\release\aegis-cli.exe aegis-cli.exe >nul
if errorlevel 1 goto failed
popd
exit /b 0
:failed
popd
exit /b 1
