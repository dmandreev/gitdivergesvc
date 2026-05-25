@echo off
setlocal

cd /d "%~dp0"

echo [1/2] Building Docker image...
docker build -t gitdiverge:latest .
if errorlevel 1 (
    echo ERROR: Docker build failed.
    exit /b 1
)

echo.
echo [2/2] Running container...
echo Stop with Ctrl+C. Container is removed automatically on stop.
echo.

docker run --rm -p 8080:8080 --name gitdiverge gitdiverge:latest
