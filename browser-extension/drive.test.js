// Google Drive URL handling: the file id is the stable identity, the
// per-session confirm token is never invented, and the virus-scan
// interstitial is never mistaken for a media response.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const R=require('./rank.js');

test('a Drive view link becomes the public download form that carries the id',()=>{
  assert.equal(R.normalizeDriveUrl('https://drive.google.com/file/d/1AbC-dEf/view'),
    'https://drive.google.com/uc?export=download&id=1AbC-dEf');
});

test('a Drive uc link gets the standard export=download form',()=>{
  assert.equal(R.normalizeDriveUrl('https://drive.google.com/uc?id=1AbC-dEf'),
    'https://drive.google.com/uc?export=download&id=1AbC-dEf');
  // Already normalized: unchanged.
  assert.equal(R.normalizeDriveUrl('https://drive.google.com/uc?export=download&id=1AbC-dEf'),
    'https://drive.google.com/uc?export=download&id=1AbC-dEf');
});

test('the signed byte endpoint keeps its confirm token',()=>{
  const signed='https://drive.usercontent.google.com/download?id=1AbC-dEf&export=download&confirm=t';
  assert.equal(R.normalizeDriveUrl(signed),signed);
});

test('extra Drive parameters survive the normalization',()=>{
  assert.equal(R.normalizeDriveUrl('https://drive.google.com/file/d/ABC/view?resourcekey=xyz'),
    'https://drive.google.com/uc?export=download&id=ABC&resourcekey=xyz');
});

test('unrelated hosts are never rewritten',()=>{
  for(const url of ['https://example.com/file.zip',
                    'https://cdn.example.com/video.mp4?token=abc&expires=1',
                    'https://googlevideo.com/videoplayback?expire=1&id=ABC',
                    'https://drive.google.com.evil.test/uc?id=ABC']){
    assert.equal(R.normalizeDriveUrl(url),url);
  }
});

test('the file id is extracted from every Drive URL shape',()=>{
  assert.equal(R.driveFileId('https://drive.google.com/uc?export=download&id=ABC'),'ABC');
  assert.equal(R.driveFileId('https://drive.google.com/file/d/ABC/view'),'ABC');
  assert.equal(R.driveFileId('https://drive.usercontent.google.com/download?id=ABC&confirm=t'),'ABC');
  assert.equal(R.driveFileId('https://docs.google.com/document/d/ABC/edit'),'ABC');
  assert.equal(R.driveFileId('https://drive.google.com/drive/folders/ABC'),null);
  assert.equal(R.driveFileId('https://example.com/file?id=ABC'),null);
});

test('the identity keeps the file id and the session uuid apart',()=>{
  const id=R.driveIdentity('https://drive.google.com/uc?export=download&id=ABC');
  assert.equal(id.id,'ABC');
  assert.equal(id.uuid,null);
  const withUuid=R.driveIdentity('https://drive.google.com/uc?export=download&id=ABC&uuid=xyz');
  assert.equal(withUuid.id,'ABC');
  assert.equal(withUuid.uuid,'xyz');
  assert.equal(R.driveIdentity('https://example.com/x'),null);
});

test('two Drive files are never the same resource',()=>{
  assert.equal(R.canonicalUrl('https://drive.google.com/uc?id=ABC'),'drive:ABC');
  assert.equal(R.canonicalUrl('https://drive.google.com/uc?export=download&id=ABC'),'drive:ABC');
  assert.equal(R.canonicalUrl('https://drive.usercontent.google.com/download?id=ABC&confirm=t'),'drive:ABC');
  assert.notEqual(R.canonicalUrl('https://drive.google.com/uc?id=ABC'),
    R.canonicalUrl('https://drive.google.com/uc?id=XYZ'));
});

test('the Drive interstitial HTML is never a media response',()=>{
  const interstitial={url:'https://drive.google.com/uc?export=download&id=ABC',type:'xmlhttprequest',
    method:'GET',statusCode:200,responseHeaders:[{name:'content-type',value:'text/html; charset=utf-8'}]};
  assert.equal(R.mediaResponse(interstitial,0),null);
  // The byte endpoint without a media name is not offered as a stream either.
  const bytes={url:'https://drive.usercontent.google.com/download?id=ABC&export=download&confirm=t',
    type:'xmlhttprequest',method:'GET',statusCode:200,
    responseHeaders:[{name:'content-type',value:'application/octet-stream'}]};
  assert.equal(R.mediaResponse(bytes,0),null);
});

test('a Drive download link is not a direct download link',()=>{
  // The endpoint can answer with the virus-scan page, so the browser keeps
  // that flow instead of handing an HTML page to the app.
  assert.equal(R.directDownloadLink('https://drive.google.com/uc?export=download&id=ABC',false),false);
  assert.equal(R.directDownloadLink('https://drive.google.com/uc?export=download&id=ABC',true),false);
  assert.equal(R.directDownloadLink('https://drive.usercontent.google.com/download?id=ABC',true),false);
  // Ordinary archives are still direct download links.
  assert.equal(R.directDownloadLink('https://example.com/file.zip',false),true);
});
