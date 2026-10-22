const {test}=require('node:test');
const assert=require('node:assert/strict');
const R=require('./rank.js');
const fixtures=require('./fixtures/detection-responses.json');
for(const f of fixtures)test('response evidence: '+f.name,()=>{
  const entry=R.mediaResponse({url:f.url,type:f.type,method:'GET',statusCode:f.status,
    responseHeaders:[{name:'Content-Type',value:f.mime}]},0);
  assert.equal(entry?.kind || null,f.expected);
  if(entry)assert.equal(entry.url,f.url);
});
test('zero size threshold is honored and partial responses use whole-file size',()=>{
  const details={url:'https://media.example.test/file.mp4',type:'media',method:'GET',statusCode:206,
    responseHeaders:[{name:'Content-Type',value:'video/mp4'},{name:'Content-Length',value:'512'},
      {name:'Content-Range',value:'bytes 0-511/2097152'}]};
  assert.equal(R.mediaResponse(details,100).size,2097152);
  details.responseHeaders.pop();
  assert.equal(R.mediaResponse(details,100),null);
  assert.equal(R.mediaResponse(details,0).size,512);
});
test('invalid URLs, credentials, unsupported schemes and non-GET observations are ignored',()=>{
  const base={type:'media',method:'GET',statusCode:200,responseHeaders:[{name:'content-type',value:'video/mp4'}]};
  for(const url of ['http://','blob:https://example.test/x','https://user:pass@example.test/v.mp4'])
    assert.equal(R.mediaResponse({...base,url},0),null);
  assert.equal(R.mediaResponse({...base,url:'https://example.test/v.mp4',method:'POST'},0),null);
});
