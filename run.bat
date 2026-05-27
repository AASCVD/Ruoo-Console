@echo off
cd /d "%~dp0"

echo ============================================================
echo   RUOO-ARSENAL // Build + Launch
echo ============================================================
echo.

:: -- Check cargo --
where cargo >nul 2>&1
if errorlevel 1 (
    echo [!] cargo not found in PATH!
    echo [*] Make sure Rust is installed.
    echo [*] Run: rustup default stable
    pause
    exit /b 1
)
echo [+] cargo: found
echo.

:: -- Build main program --
echo [1/1] cargo build --release --bin ruoo-console...
cargo build --release --bin ruoo-console
if errorlevel 1 (
    echo.
    echo [!] ruoo-console build FAILED! See errors above.
    pause
    exit /b 1
)
echo [+] ruoo-console built OK
echo.

:: -- Launch --
set "EXE=target\release\ruoo-console.exe"
if not exist "%EXE%" (
    echo [!] Exe not found: %EXE%
    pause
    exit /b 1
)
echo [*] EXE: %EXE%

echo ============================================================
echo   Launching...
echo ============================================================
echo.

"%EXE%" 2>&1

echo.
echo ============================================================
echo   Exit code: %errorlevel%
echo ============================================================
pause
