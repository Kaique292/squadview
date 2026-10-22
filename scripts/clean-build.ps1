<#
.SYNOPSIS
  Mostra o tamanho das pastas de build do SquadView e limpa o que for pedido.

.DESCRIPTION
  Sem parâmetros, só mostra: a target/ do projeto (e as subpastas grandes) e a target/ de cada
  worktree em .claude/worktrees. Só apaga com -Clean, -Coverage, -Worktrees ou -IfOverGB, e nunca
  apaga nada fora de uma pasta target/.

.PARAMETER Clean
  cargo clean: apaga a target/ do projeto inteira (o próximo build compila tudo de novo).

.PARAMETER Coverage
  Apaga a target/llvm-cov-target inteira, só o que o llvm-cov gerou (o resto da target/ fica).
  O `cargo llvm-cov clean --workspace` não serve: ele deixa as dependências instrumentadas, que
  são quase tudo.

.PARAMETER Worktrees
  Apaga a target/ de cada worktree em .claude/worktrees (o resto do worktree fica).

.PARAMETER IfOverGB
  Faz o -Clean só se a target/ do projeto passar desse tamanho (ex.: -IfOverGB 20).

.EXAMPLE
  ./scripts/clean-build.ps1
  ./scripts/clean-build.ps1 -IfOverGB 20
  ./scripts/clean-build.ps1 -Coverage -Worktrees
#>
param(
  [switch]$Clean,
  [switch]$Coverage,
  [switch]$Worktrees,
  [double]$IfOverGB = 0
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "target"

function Get-SizeGB([string]$path) {
  if (-not (Test-Path $path)) { return 0 }
  $bytes = (Get-ChildItem $path -Recurse -Force -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
  return [math]::Round(($bytes / 1GB), 2)
}

function Show([string]$label, [double]$gb) {
  "{0,9:N2} GB  {1}" -f $gb, $label
}

"Pastas de build do SquadView:"
$total = Get-SizeGB $target
Show "target/" $total
if (Test-Path $target) {
  Get-ChildItem $target -Directory -Force | ForEach-Object {
    $gb = Get-SizeGB $_.FullName
    if ($gb -ge 0.5) { Show "  target/$($_.Name)" $gb }
  }
}

$worktreeTargets = @()
$worktreesDir = Join-Path $root ".claude/worktrees"
if (Test-Path $worktreesDir) {
  Get-ChildItem $worktreesDir -Directory -Force | ForEach-Object {
    $wt = Join-Path $_.FullName "target"
    if (Test-Path $wt) {
      $worktreeTargets += $wt
      Show ".claude/worktrees/$($_.Name)/target/" (Get-SizeGB $wt)
    }
  }
}

$drive = (Get-Item $root).PSDrive
"Livre no disco {0}: {1:N1} GB" -f $drive.Name, ($drive.Free / 1GB)

Push-Location $root
try {
  if ($IfOverGB -gt 0 -and $total -gt $IfOverGB) {
    "target/ passou de $IfOverGB GB: limpando."
    $Clean = $true
  }
  $coverageTarget = Join-Path $target "llvm-cov-target"
  if ($Coverage -and -not $Clean -and (Test-Path $coverageTarget)) {
    "Apagando $coverageTarget"
    Remove-Item $coverageTarget -Recurse -Force
  }
  if ($Clean) {
    cargo clean
  }
  if ($Worktrees) {
    foreach ($wt in $worktreeTargets) {
      # Só a target/ do worktree; o código e o histórico dele ficam.
      "Apagando $wt"
      Remove-Item $wt -Recurse -Force
    }
  }
} finally {
  Pop-Location
}
