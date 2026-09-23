param(
    [Parameter(Mandatory = $true)]
    [string]$Port,

    [int]$Iterations = 500,
    [int]$OpenMilliseconds = 20,
    [int]$NoReadEvery = 25,
    [int]$NoReadMilliseconds = 500,
    [int]$BaudRate = 115200
)

$ErrorActionPreference = "Stop"

if ($Iterations -lt 1) {
    throw "Iterations must be >= 1"
}
if ($NoReadEvery -lt 0) {
    throw "NoReadEvery must be >= 0"
}

$payload = [System.Text.Encoding]::ASCII.GetBytes("TauTerm-vport-stress`r`n")
$opened = 0
$bytesWritten = 0
$bytesRead = 0

Write-Host "TauTerm VPort stress: port=$Port iterations=$Iterations baud=$BaudRate"
Write-Host "Keep the owning TauTerm Serial Session connected while this script runs."

for ($i = 1; $i -le $Iterations; $i++) {
    $serial = [System.IO.Ports.SerialPort]::new(
        $Port,
        $BaudRate,
        [System.IO.Ports.Parity]::None,
        8,
        [System.IO.Ports.StopBits]::One
    )
    $serial.ReadTimeout = 25
    $serial.WriteTimeout = 250

    try {
        $serial.Open()
        $opened++

        $serial.Write($payload, 0, $payload.Length)
        $bytesWritten += $payload.Length

        $holdWithoutReading = $NoReadEvery -gt 0 -and ($i % $NoReadEvery -eq 0)
        if ($holdWithoutReading) {
            Start-Sleep -Milliseconds $NoReadMilliseconds
        }
        else {
            Start-Sleep -Milliseconds $OpenMilliseconds
            $available = $serial.BytesToRead
            if ($available -gt 0) {
                $buffer = New-Object byte[] $available
                $read = $serial.Read($buffer, 0, $available)
                $bytesRead += $read
            }
        }
    }
    finally {
        if ($serial.IsOpen) {
            $serial.Close()
        }
        $serial.Dispose()
    }

    if (($i % 50) -eq 0 -or $i -eq $Iterations) {
        Write-Host ("progress {0}/{1}: opens={2}, tx={3} B, rx={4} B" -f $i, $Iterations, $opened, $bytesWritten, $bytesRead)
    }
}

Write-Host "PASS: completed $opened external COM open/write/close cycles without a peer-side exception."
Write-Host "Inspect TauTerm System Log/status for degraded/backpressured recovery and verify the parent Session stayed alive."
