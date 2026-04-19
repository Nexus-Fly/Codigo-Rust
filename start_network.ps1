# NexusFly – Lanzador de 3 nodos reales con Tashi Vertex BFT
# Ejecutar desde: C:\Hackatons\Nexus-Fly\codigo_rust

$root = $PSScriptRoot
$exe  = "$root\target\debug\codigo_rust.exe"
$dll  = "$root\target\debug"

# ── Keypairs generados (no reutilizar en produccion) ───────────────────────
$SK1 = "3d1RiRMXUVXc7RnBau79SciJZMxNdFKGy39b4WihsBLTtYiPXepaJdUzkvmkJYcNkjgND4"
$PK1 = "aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTJppiXibvr83fHFnzKQme5JjY9SvSE4fE49eaM2zU1g9iKukN8eC7xXrLZev3H5bQridEzApaNfS1UJQTpEHR9WkY"

$SK2 = "3d1RiRMXUUnwZQPcUgczdFUPwEiokqX4WQWqY8qoMcKKDyu3q3k7cWKoUUiTRT4ac88kyc"
$PK2 = "aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTFLXLV68GwRMSLtEYFMATpcTgetidcTuLMoGeiom9jyiYZJCmnwkxZTcn8R5Q1QWFbt6kLeXuXvM3rEPFBCrSJWkM"

$SK3 = "3d1RiRMXUVxdvaXs6fLFaY3KvNuNAuY79c1uEKDfLEHXXVjH46UPQnXd7J1bCSqNsfp5re"
$PK3 = "aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTGjHJDBK8G8NbjeGqtJwQqmthZsfgHNTWZftneh7d3jBCYkhQxPbRx6sM3CmkgQpVXoi7eMxah8jptDF2GMPntjEp"

# ── Comandos por nodo ──────────────────────────────────────────────────────
$cmd1 = "& '$exe' --secret-key '$SK1' --bind-addr '127.0.0.1:9000' --peer '${PK2}@127.0.0.1:9001' --peer '${PK3}@127.0.0.1:9002' --agent-id drone-001 --agent-type drone --balance 100; pause"
$cmd2 = "& '$exe' --secret-key '$SK2' --bind-addr '127.0.0.1:9001' --peer '${PK1}@127.0.0.1:9000' --peer '${PK3}@127.0.0.1:9002' --agent-id robot-002 --agent-type robot --balance 50; pause"
$cmd3 = "& '$exe' --secret-key '$SK3' --bind-addr '127.0.0.1:9002' --peer '${PK1}@127.0.0.1:9000' --peer '${PK2}@127.0.0.1:9001' --agent-id ebike-003 --agent-type ebike --balance 75; pause"

Write-Host "Verificando que el binario existe..."
if (-not (Test-Path $exe)) {
    Write-Error "No encontre $exe. Ejecuta 'cargo build' primero."
    exit 1
}

Write-Host "Abriendo 3 ventanas de PowerShell (nodos drone-001, robot-002, ebike-003)..."
$env:PATH = "$dll;" + $env:PATH

Start-Process powershell -ArgumentList "-NoExit", "-Command", "& { `$env:PATH = '$dll;' + `$env:PATH; $cmd1 }" -WindowStyle Normal
Start-Sleep -Milliseconds 500
Start-Process powershell -ArgumentList "-NoExit", "-Command", "& { `$env:PATH = '$dll;' + `$env:PATH; $cmd2 }" -WindowStyle Normal
Start-Sleep -Milliseconds 500
Start-Process powershell -ArgumentList "-NoExit", "-Command", "& { `$env:PATH = '$dll;' + `$env:PATH; $cmd3 }" -WindowStyle Normal

Write-Host "Listo. Se abrieron 3 ventanas. Espera ~2s a que se conecten entre si."
