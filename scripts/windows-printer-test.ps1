# Windows only. Proves printing end to end on a machine with no printer: it creates a receipt printer
# that writes to a file, prints a receipt to it with the Node client, and checks the bytes.
#
#   cargo build
#   powershell -ExecutionPolicy Bypass -File scripts\windows-printer-test.ps1
#
# Needs an administrator shell (to add a printer) and Node.js. In CI it runs as-is.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$work = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$out = Join-Path $work 'pdf-engine-receipt.prn'
$name = 'pdf-engine test POS-58'
Remove-Item $out -ErrorAction SilentlyContinue

# A local port named after a file path writes everything sent to it into that file.
if (-not (Get-PrinterPort -Name $out -ErrorAction SilentlyContinue)) { Add-PrinterPort -Name $out }
if (-not (Get-PrinterDriver -Name 'Generic / Text Only' -ErrorAction SilentlyContinue)) { Add-PrinterDriver -Name 'Generic / Text Only' }
if (Get-Printer -Name $name -ErrorAction SilentlyContinue) { Remove-Printer -Name $name }
Add-Printer -Name $name -DriverName 'Generic / Text Only' -PortName $out
Write-Host "Added printer '$name' writing to $out"

try {
  $env:PDF_ENGINE_TEST_PRINTER = $name
  $env:PDF_ENGINE_TEST_OUT = $out
  node (Join-Path $PSScriptRoot 'windows-printer-test.js')
  if ($LASTEXITCODE -ne 0) { throw "the print test failed (exit $LASTEXITCODE)" }
} finally {
  Remove-Printer -Name $name -ErrorAction SilentlyContinue
  Remove-PrinterPort -Name $out -ErrorAction SilentlyContinue
}
