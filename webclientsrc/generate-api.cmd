@echo off
echo Generating GitDiverge API client from OpenAPI spec...
cd /d "%~dp0"
..\target\release\gitdiverge.exe openapi -o divergeapi.json 
npx @hey-api/openapi-ts -f openapi-ts.config.ts
echo Done.
