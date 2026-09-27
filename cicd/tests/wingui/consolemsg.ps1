##	A release build owns no console, so what a control command has to say goes
##	to the console it was typed at, or nowhere. It went nowhere for a while.
##	Test ID: EqH4isq

$said = Join-Path $OutDir "consolemsg.txt"
$c = Start-Process cmd.exe -ArgumentList '/k', "`"$Exe`" --reload-settings" -PassThru
fTrack $c
Start-Sleep -Seconds 3
$reader = (Get-Process -Id $PID).Path
Start-Process $reader -ArgumentList '-NoProfile', '-File', "`"$PSScriptRoot\_readcon.ps1`"", '-Of', $c.Id, '-Out', "`"$said`"" -WindowStyle Hidden -Wait
$text = if (Test-Path $said) { (Get-Content $said -Raw) -replace '\s+', ' ' } else { '' }
fNote ("console reads: " + $text.Trim())
[void](fCheck "the console could be read" ($text -ne '' -and $text -notlike '*could not attach*'))
[void](fCheck "--reload-settings says why it did nothing, on the console it was typed at" ($text -like '*SilkTerm: *'))
fStop $c
