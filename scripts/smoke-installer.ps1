# Teste de fumaça do instalador: faz o caminho de quem instala e de quem atualiza, com o app de
# verdade, e falha se o app cair (crash.log) ou não ficar aberto.
#
# Foi o caminho que quebrou duas vezes: na 0.2.0 o app não abria depois de atualizar, e na 0.3.0
# ele caía porque a atualização abre o app duas vezes quase juntas. Roda na esteira (job
# "Instalador de teste" e na release, antes de publicar). Uso:
#
#   pwsh scripts/smoke-installer.ps1 [caminho-do-instalador.exe]
#
# Instala no perfil do usuário atual e fecha o SquadView aberto: não rode no seu PC de uso.

param([string]$Installer)

$ErrorActionPreference = "Stop"
if (-not $Installer) {
  $Installer = Get-ChildItem "target/release/bundle/nsis/*-setup.exe" | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $Installer -or -not (Test-Path $Installer)) { throw "Instalador não encontrado." }

$logs = Join-Path $env:LOCALAPPDATA "io.github.kaique292.squadview\logs"
$crashLog = Join-Path $logs "crash.log"
# Quanto tempo o app precisa ficar de pé depois de cada abertura.
$settle = 15

function Stop-SquadView {
  Get-Process squadview -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Seconds 2
}

function Find-App {
  $candidates = @(
    (Join-Path $env:LOCALAPPDATA "SquadView\squadview.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\SquadView\squadview.exe")
  )
  $app = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
  if (-not $app) { throw "Depois de instalar, o squadview.exe não apareceu em: $($candidates -join ', ')" }
  return $app
}

# O app tem de estar aberto e sem crash.log.
function Assert-Alive([string]$step) {
  Start-Sleep -Seconds $settle
  if (Test-Path $crashLog) {
    Write-Host "::error::$step : o app caiu."
    Get-Content $crashLog | Write-Host
    exit 1
  }
  $running = @(Get-Process squadview -ErrorAction SilentlyContinue)
  if ($running.Count -eq 0) {
    Write-Host "::error::$step : o app não ficou aberto."
    if (Test-Path $logs) { Get-ChildItem $logs | ForEach-Object { Write-Host "--- $($_.Name)"; Get-Content $_.FullName -Tail 20 | Write-Host } }
    exit 1
  }
  Write-Host "ok - $step ($($running.Count) processo(s) do SquadView)"
}

Stop-SquadView
Remove-Item $crashLog -ErrorAction SilentlyContinue

# 1. Primeira instalação: dois cliques, sem assistente. O instalador abre o app sozinho.
Start-Process $Installer -ArgumentList "/S" -Wait
$app = Find-App
Assert-Alive "instalação silenciosa abre o app"

# 2. Abrir de novo com o app aberto (atalho, convite): a segunda instância avisa a primeira.
Start-Process $app
Assert-Alive "abrir de novo com o app aberto"

# 3. Duas aberturas quase juntas, com o app fechado: a segunda chega antes de a primeira terminar
#    de abrir (o crash da 0.3.0).
Stop-SquadView
Start-Process $app
Start-Sleep -Milliseconds 150
Start-Process $app
Assert-Alive "duas aberturas quase juntas"

# 4. Atualização como o updater faz: o app fecha, o instalador roda com /UPDATE /R (e reabre o
#    app), e o app antigo ainda pede para reiniciar logo depois.
Stop-SquadView
Start-Process $Installer -ArgumentList "/S", "/UPDATE", "/R" -Wait
Start-Process $app
Assert-Alive "atualização reabre o app"

Stop-SquadView
Write-Host "Teste de fumaça do instalador: tudo certo."
