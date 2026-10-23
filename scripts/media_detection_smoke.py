"""Generate original test media and probe an isolated Edge extension profile.

No reference archive code is loaded. All network resources are loopback.
The test-only manifest omits nativeMessaging, so the desktop is not invoked.
Run: python scripts/media_detection_smoke.py --edge <path-to-msedge.exe>
"""
import argparse
import functools
import hashlib
import http.server
import json
import pathlib
import shutil
import subprocess
import threading
import time
from package_extensions import stage_extensions


def run():
    parser = argparse.ArgumentParser()
    parser.add_argument("--edge", type=pathlib.Path, required=True)
    args = parser.parse_args()
    repo = pathlib.Path(__file__).resolve().parents[1]
    output = repo / "dist" / ("phase3-edge-smoke-" + str(time.time_ns()))
    media = output / "media"
    media.mkdir(parents=True)
    flags = {"creationflags": subprocess.CREATE_NO_WINDOW} if hasattr(subprocess, "CREATE_NO_WINDOW") else {}
    def command(values):
        result = subprocess.run(values, check=True, capture_output=True, text=True, timeout=45, cwd=media, **flags)
        return result.stdout
    ffmpeg = shutil.which("ffmpeg")
    ffprobe = shutil.which("ffprobe")
    if not ffmpeg or not ffprobe:
        raise RuntimeError("Install FFmpeg/ffprobe to generate original non-DRM fixture media")
    command([ffmpeg, "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
             "testsrc2=size=160x90:rate=10", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100",
             "-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-movflags", "+faststart", str(media / "sample.mp4")])
    command([ffmpeg, "-hide_banner", "-loglevel", "error", "-i", str(media / "sample.mp4"), "-vn", "-c:a", "pcm_s16le", str(media / "sample.wav")])
    command([ffmpeg, "-hide_banner", "-loglevel", "error", "-i", str(media / "sample.mp4"), "-c", "copy",
             "-hls_time", "1", "-hls_list_size", "0", str(media / "sample.m3u8")])
    command([ffmpeg, "-hide_banner", "-loglevel", "error", "-i", str(media / "sample.mp4"), "-map", "0", "-c", "copy", "-f", "dash", str(media / "sample.mpd")])
    probe = json.loads(command([ffprobe, "-v", "error", "-show_entries", "stream=codec_type", "-of", "json", str(media / "sample.mp4")]))
    assert {s["codec_type"] for s in probe["streams"]} == {"audio", "video"}
    requests = []
    class Handler(http.server.SimpleHTTPRequestHandler):
        extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                          ".m3u8": "application/vnd.apple.mpegurl", ".mpd": "application/dash+xml"}
        def log_message(self, *_):
            pass
        def do_GET(self):
            requests.append(self.path)
            if self.path.startswith(("/binary", "/fake.mp4", "/denied.mp4")):
                self.send_response(403 if self.path.startswith("/denied") else 200)
                self.send_header("Content-Type", "application/octet-stream" if self.path.startswith("/binary") else "text/html")
                self.end_headers()
                self.wfile.write(b"<p>Not playable media</p>")
                return
            try:
                super().do_GET()
            except ConnectionError:
                pass  # Media clients may close a response after probing it.
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(media)))
    port = server.server_port
    (media / "index.html").write_text(f'''<!doctype html><title>Original media fixtures</title>
<video controls width="320" height="180" src="/sample.mp4"></video>
<audio controls src="/sample.wav"></audio>
<iframe src="http://localhost:{port}/frame.html" width="400" height="250"></iframe>
<script src="/requests.js"></script>''', encoding="utf-8")
    (media / "frame.html").write_text('''<!doctype html><title>Permitted embedded player</title>
<video controls width="320" height="180" src="/sample.mp4?frame=embedded"></video>''', encoding="utf-8")
    (media / "requests.js").write_text('''for(const path of ["/sample.m3u8?token=exact", "/sample.mpd", "/binary", "/fake.mp4", "/denied.mp4"])
fetch(path).then(r=>r.text()).catch(()=>{});''', encoding="utf-8")
    chrome, _, _ = stage_extensions(repo, output / "extensions")
    manifest_path = chrome / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["permissions"].remove("nativeMessaging")
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    (chrome / "smoke.html").write_text('<!doctype html><pre id="result">pending</pre><script src="smoke.js"></script>', encoding="utf-8")
    (chrome / "smoke.js").write_text('''(async()=>{
try {
document.body.dataset.step="storage";
await chrome.storage.local.set({minSizeKB:0,autoCapture:false});
document.body.dataset.step="create-tab";
const tab=await chrome.tabs.create({url:FIXTURE,active:true});
document.body.dataset.step="wait-for-fixtures";
await new Promise(resolve=>setTimeout(resolve,4000));
document.body.dataset.step="query-media";
const reply=await chrome.runtime.sendMessage({type:"getMedia",tabId:tab.id});
await chrome.scripting.executeScript({target:{tabId:tab.id},func:()=>{
  document.querySelector('audio').src='/sample.wav?revision=2';
}});
await new Promise(resolve=>setTimeout(resolve,1000));
document.body.dataset.step="inspect-frames";
const frames=await chrome.scripting.executeScript({target:{tabId:tab.id,allFrames:true},func:()=>
  [...document.querySelectorAll("[data-dc-pill]")].map(el=>el.shadowRoot?.querySelector(".label")?.textContent)});
const dynamic=await chrome.scripting.executeScript({target:{tabId:tab.id},func:()=>document.querySelector('audio').__dcKey});
document.getElementById("result").textContent=JSON.stringify({items:reply.items,frames:frames.map(f=>({frameId:f.frameId,labels:f.result})),dynamicKey:dynamic[0].result});
document.body.dataset.step="complete";
}catch(e){document.getElementById("result").textContent=JSON.stringify({error:String(e)});}
})();'''.replace("FIXTURE", json.dumps(f"http://127.0.0.1:{port}/index.html")), encoding="utf-8")
    # Pinned public key gives the same extension ID as the ordinary package.
    import base64
    digest = hashlib.sha256(base64.b64decode(manifest["key"])).hexdigest()[:32]
    extension_id = "".join(chr(ord("a") + int(c, 16)) for c in digest)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    result = {"browser":str(args.edge), "output":str(output), "sampleProbe":probe,
              "scope":"isolated native extension detection; native messaging disabled"}
    try:
        stderr_path = output / "browser-stderr.log"
        with stderr_path.open("w", encoding="utf-8") as stderr:
            proc = subprocess.Popen([str(args.edge), "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
            "--disable-background-networking", "--disable-component-update", "--disable-sync", "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding", f"--user-data-dir={output / 'profile'}", f"--disable-extensions-except={chrome}",
            f"--load-extension={chrome}", "--remote-debugging-port=0", f"chrome-extension://{extension_id}/smoke.html"],
                stdout=subprocess.DEVNULL, stderr=stderr, **flags)
            try:
                port_file = output / "profile" / "DevToolsActivePort"
                for _ in range(100):
                    if port_file.exists():
                        break
                    time.sleep(.1)
                if not port_file.exists():
                    raise RuntimeError("Isolated browser debugging endpoint unavailable")
                debug_port = port_file.read_text().splitlines()[0]
                observation = json.loads(command([shutil.which("node") or "node", str(repo / "scripts/media_detection_cdp.js"), debug_port, extension_id, str(chrome / "smoke.js")]))
            finally:
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.terminate()
                    proc.wait(timeout=5)
        result["observation"] = observation
        items = observation.get("items", [])
        urls = [item["url"] for item in items]
        labels = [label for frame in observation.get("frames", []) for label in frame["labels"]]
        result["checks"] = {
            "directVideo":any("sample.mp4" in u and "frame=" not in u for u in urls),
            "audio":any("sample.wav" in u for u in urls),
            "embeddedVideo":any("frame=embedded" in u for u in urls),
            "hls":any("sample.m3u8?token=exact" in u for u in urls),
            "dash":any("sample.mpd" in u for u in urls),
            "falsePositivesExcluded":bool(items) and not any(bad in u for u in urls for bad in ["/binary", "/fake.mp4", "/denied.mp4"]),
            "audioControl":"Download this audio" in labels,
            "videoControl":"Download this video" in labels,
            "dynamicAudioControl":"revision=2" in observation.get("dynamicKey", ""),
        }
        result["passed"] = all(result["checks"].values())
    except (subprocess.SubprocessError, RuntimeError) as error:
        result.update(passed=False, error=str(error), diagnostic=getattr(error, "stderr", None))
    finally:
        server.shutdown()
        server.server_close()
    result["fixtureRequests"] = requests
    (output / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(run())
