$ErrorActionPreference = 'Stop'

$gpui = Get-ChildItem -Path vendor -Directory -Filter 'gpui-0.2.2' | Select-Object -First 1
if (-not $gpui) {
    throw 'vendored gpui-0.2.2 was not found'
}

$windowsDir = Join-Path $gpui.FullName 'src\platform\windows'
$devicesPath = Join-Path $windowsDir 'directx_devices.rs'
$rendererPath = Join-Path $windowsDir 'directx_renderer.rs'
$destinationListPath = Join-Path $windowsDir 'destination_list.rs'

foreach ($path in @($devicesPath, $rendererPath, $destinationListPath)) {
    if (-not (Test-Path $path)) {
        throw "expected GPUI source file was not found: $path"
    }
}

# Windows Server 2016 / Windows 10 1607 expose DXGI 1.2 but not IDXGIFactory6
# (DXGI 1.6 / Windows 10 1803). GPUI 0.2.2 only uses methods already available
# on IDXGIFactory2, so lower the interface requirement without changing rendering behavior.
$devices = Get-Content -Raw $devicesPath
if ($devices -notmatch 'IDXGIFactory6') {
    throw 'unexpected GPUI directx_devices.rs: IDXGIFactory6 marker missing'
}
$devices = $devices.Replace('IDXGIFactory6', 'IDXGIFactory2')
Set-Content -Path $devicesPath -Value $devices -Encoding utf8NoBOM

$renderer = Get-Content -Raw $rendererPath
if ($renderer -notmatch 'IDXGIFactory6') {
    throw 'unexpected GPUI directx_renderer.rs: IDXGIFactory6 marker missing'
}
$renderer = $renderer.Replace('IDXGIFactory6', 'IDXGIFactory2')
Set-Content -Path $rendererPath -Value $renderer -Encoding utf8NoBOM

# Windows 10 1703+ ships ICU as a system DLL, while Server 2016 / Win10 1607 do not.
# GPUI 0.2.2 uses ICU only for u_strlen in destination_list.rs. Replace that call
# with a bounded Rust scan so the legacy executable has no icuuc.dll dependency at all.
$destination = Get-Content -Raw $destinationListPath
if ($destination -notmatch 'u_strlen') {
    throw 'unexpected GPUI destination_list.rs: u_strlen marker missing'
}
$destination = [regex]::Replace(
    $destination,
    '(?m)^\s*Globalization::u_strlen,\r?\n',
    ''
)
$destination = $destination.Replace(
    'let len = unsafe { u_strlen(buffer.as_ptr()) };',
    'let len = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());'
)
$destination = $destination.Replace('&buffer[..len as usize]', '&buffer[..len]')
if ($destination -match 'u_strlen') {
    throw 'legacy GPUI patch did not fully remove u_strlen'
}
Set-Content -Path $destinationListPath -Value $destination -Encoding utf8NoBOM

Write-Host "Patched GPUI 0.2.2 for Windows Server 2016 / Windows 10 1607"
