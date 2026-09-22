# clipboard-bridge.ps1 -- serve the Windows clipboard's image to a cctop on
# the far side of an ssh connection.
#
# A terminal cannot carry an image, so over ssh the clipboard is out of reach --
# but the ssh connection carries sockets back down it fine. Run this on the
# Windows side and forward the port when connecting:
#
#   ssh -R 8377:127.0.0.1:8377 <host>
#
# or once, in ~/.ssh/config:
#
#   RemoteForward 8377 127.0.0.1:8377
#
# Then F9 -- and Ctrl+V in a pane -- paste the clipboard's image as usual, in
# Windows Terminal or anywhere else. cctop asks by connecting; the answer is
# the PNG, or a closed connection when the clipboard holds no image. The port
# can be anything both ends agree on: -Port here, CCTOP_CLIPBOARD_PORT there.
#
# Windows PowerShell reads a .ps1 without a BOM in the system's ANSI codepage,
# so this file stays ASCII on purpose.
param([int]$Port = 8377)

# The clipboard API wants a single-threaded apartment. powershell.exe -File is
# already one, but under pwsh or another host this relaunches into one.
if ([Threading.Thread]::CurrentThread.ApartmentState -ne 'STA') {
    powershell.exe -STA -NoProfile -File $PSCommandPath -Port $Port
    exit $LASTEXITCODE
}

Add-Type -AssemblyName System.Windows.Forms,System.Drawing

$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
$listener.Start()
Write-Host "clipboard bridge on 127.0.0.1:$Port - ssh -R ${Port}:127.0.0.1:${Port} <host>"

while ($true) {
    $client = $listener.AcceptTcpClient()
    try {
        # A clipboard that is mid-copy in another program throws rather than
        # answers; the connection closing empty is how "no image" is said.
        $image = [System.Windows.Forms.Clipboard]::GetImage()
        if ($null -ne $image) {
            $ms = New-Object System.IO.MemoryStream
            $image.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $bytes = $ms.ToArray()
            $client.GetStream().Write($bytes, 0, $bytes.Length)
            $ms.Dispose()
            $image.Dispose()
        }
    } finally {
        $client.Close()
    }
}
