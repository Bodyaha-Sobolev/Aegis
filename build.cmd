@echo off
rem Aegis: language workspace (language/) and Sentinel IDE workspace (ide/).
setlocal
pushd "%~dp0"
set "AEGIS_PYTHON=py"
where py >nul 2>nul
if errorlevel 1 set "AEGIS_PYTHON=python"
if /I "%~1"=="test" (
    %AEGIS_PYTHON% language\tools\verify_vendor.py
    if errorlevel 1 goto failed
    cargo fmt --manifest-path language\Cargo.toml --package aegis-lang --package aegis-cli -- --check
    if errorlevel 1 goto failed
    cargo fmt --manifest-path ide\Cargo.toml --package sentinel -- --check
    if errorlevel 1 goto failed
    cargo clippy --locked --manifest-path language\Cargo.toml --workspace --all-targets -- -D warnings
    if errorlevel 1 goto failed
    cargo clippy --locked --manifest-path ide\Cargo.toml --workspace --all-targets -- -D warnings
    if errorlevel 1 goto failed
    cargo test --locked --manifest-path language\Cargo.toml --workspace
    if errorlevel 1 goto failed
    cargo test --locked --manifest-path ide\Cargo.toml --workspace
    if errorlevel 1 goto failed
    cargo build --locked --manifest-path language\Cargo.toml -p aegis-cli
    if errorlevel 1 goto failed
    %AEGIS_PYTHON% language\tools\test_native.py
    if errorlevel 1 goto failed
    %AEGIS_PYTHON% language\tools\test_windows_abi.py
    if errorlevel 1 goto failed
    %AEGIS_PYTHON% language\tools\test_ffi.py
    if errorlevel 1 goto failed
    %AEGIS_PYTHON% language\tools\test_enums.py
    if errorlevel 1 goto failed
    %AEGIS_PYTHON% tools\audit_dependencies.py
    if errorlevel 1 goto failed
    popd
    exit /b 0
)
cargo build --locked --release --manifest-path language\Cargo.toml --workspace
if errorlevel 1 goto failed
cargo build --locked --release --manifest-path ide\Cargo.toml --workspace
if errorlevel 1 goto failed
if not exist dist mkdir dist
copy /Y ide\target\release\sentinel.exe dist\sentinel.exe >nul
if errorlevel 1 goto failed
copy /Y language\target\release\aegis-cli.exe dist\aegis-cli.exe >nul
if errorlevel 1 goto failed
copy /Y language\THIRD_PARTY_NOTICES.txt dist\THIRD_PARTY_NOTICES.txt >nul
if errorlevel 1 goto failed
popd
exit /b 0
:failed
popd
exit /b 1
