// The file name that travels with a capture is untrusted: it comes from the
// page (an anchor's `download` attribute, or the browser's own saved name)
// and it is handed to the app, where it can end up on a command line other
// local users can read. Only a plain single file name may pass.
const vm=require('node:vm');
const fs=require('node:fs');
const assert=require('node:assert');
const test=require('node:test');

const source=fs.readFileSync(__dirname+'/background.js','utf8');
const start=source.indexOf('function safeCaptureFilename(');
assert.ok(start>0,'the background worker must have a file name check');
const slice=source.slice(start,source.indexOf('// ---- IDM-style network sniffer ----'));
const context={};
vm.createContext(context);
vm.runInContext(slice,context);
const safe=context.safeCaptureFilename;

test('a plain file name is passed on unchanged',()=>{
  assert.equal(safe('clip.mp4'),'clip.mp4');
  assert.equal(safe('  clip.mp4  '),'clip.mp4');
  assert.equal(safe('Artist - Track.mp3'),'Artist - Track.mp3');
  assert.equal(safe('فارسی.mp3'),'فارسی.mp3');
});

test('a name that is a path, or a control character, is refused',()=>{
  assert.equal(safe('..\\..\\Startup\\evil.exe'),null);
  assert.equal(safe('/etc/passwd'),null);
  assert.equal(safe('dir/file.mp4'),null);
  assert.equal(safe('bad\u0000name.mp4'),null);
  assert.equal(safe('line\nbreak.mp4'),null);
  assert.equal(safe(''),null);
  assert.equal(safe('   '),null);
  assert.equal(safe(undefined),null);
  assert.equal(safe(42),null);
  assert.equal(safe('x'.repeat(256)),null);
});

test('the name sent to the native host is the checked one',()=>{
  const fnStart=source.indexOf('async function sendToHost(');
  assert.ok(fnStart>0,'the background worker must send the capture to the host');
  const fn=source.slice(fnStart,source.indexOf('\n}',fnStart));
  assert.ok(
    /filename:\s*safeCaptureFilename\(filename\)\s*\|\|\s*null/.test(fn),
    'the host message must carry the checked file name'
  );
});
