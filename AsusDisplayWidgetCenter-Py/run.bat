@echo off
setlocal
cd /d "%~dp0"
where pythonw >nul 2>nul
if %errorlevel%==0 (
    start "" pythonw "%~dp0main.py" %*
) else (
    python "%~dp0main.py" %*
)
endlocal
