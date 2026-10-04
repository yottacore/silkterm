##	A window under GPU load with Free resources when idle on and a short idle
##	time, so the device is let go and built again under load. See _gpuload.ps1.
##	Not in the pipeline: it needs the load program sent along.
##	Test ID: ErgpmyB

$idleRelease = $true
. "$PSScriptRoot\_gpuload.ps1"
