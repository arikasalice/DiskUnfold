// F01 범위 편집 화면의 제외 규칙 입력 보드. 12번 페이지에서 mcp__penpot__execute_code로 실행한다(openPage는 별도 호출로 먼저).
// 보드 자리: x=19760, Light y=0, Dark y=1040. 같은 이름의 보드가 있으면 지우고 다시 그린다.

const R=(x,y,w,h,f,o={})=>({k:'r',x,y,w,h,f,...o});
const Tx=(x,y,t,o={})=>({k:'t',x,y,t,...o});
const navs=['새 분석','공간 탐색','시점 비교','중복 검토','정리 계획','보관함','작업 이력','설정'];
const shell=(nav,title,sub,theme)=>{const a=[R(0,0,1440,40,'chrome'),Tx(24,11,'DiskUnfold  /  내 작업공간',{z:12,c:'muted'}),Tx(1320,11,'—     □     ×',{z:12,c:'muted'}),R(0,40,212,920,'chrome'),Tx(24,68,'DiskUnfold',{z:24,b:700}),Tx(24,111,'공간을 펼쳐 보다',{z:12,c:'muted'})];
 navs.forEach((n,i)=>{const y=172+i*48; if(n===nav){a.push(R(12,y-7,188,40,'sel',{r:7}),Tx(28,y,n,{z:14,b:700,c:'accent'}));} else a.push(Tx(28,y,n,{z:14,c:'muted'}));});
 a.push(Tx(24,832,'로컬에서 분석',{z:12,c:'muted'}),Tx(24,850,'한국어 · '+(theme==='light'?'Light':'Dark'),{z:12,c:'muted'}));
 if(title) a.push(Tx(248,70,title,{z:28,b:700})); if(sub) a.push(Tx(248,118,sub,{z:13,c:'muted'}));
 a.push(R(248,905,1156,1,'line'),Tx(248,920,'예시 데이터  ·  설계 기준: docs/handoff/2026-11-14/design/F01/README.md  ·  키보드로 모든 기능을 사용할 수 있습니다',{z:11,c:'muted'})); return a;};
const btn=(x,y,w,label,kind='secondary')=>{const m={primary:['accent','onAccent',null],secondary:['surface','text','line'],disabled:['chrome','muted','line'],focus:['surface','text','accent']}[kind]; return [R(x,y,w,38,m[0],{r:7,s:m[2],sw:kind==='focus'?2:1,n:'Button / '+label}),Tx(x+12,y+9,label,{z:13,b:500,c:m[1]})];};
const btnS=(x,y,w,label)=>[R(x,y,w,28,'surface',{r:6,s:'line',n:'RowButton / '+label}),Tx(x+10,y+6,label,{z:12,b:500})];
const card=(x,y,w,h,o={})=>R(x,y,w,h,'surface',{r:10,...o});
const thead=(x,y,w,cols)=>[R(x,y,w,36,'chrome',{r:5}),...cols.map(([cx,t])=>Tx(x+cx,y+9,t,{z:12,b:500,c:'muted'}))];
const row=(x,y,w,cols,h=40)=>[R(x,y,w,h,'surface'),R(x,y+h,w,1,'line',{o:0.5}),...cols.map(([cx,t,o])=>Tx(x+cx,y+(h-20)/2+2,t,{z:13,...(o||{})}))];
const seg=(x,y,w,opts,act)=>{const a=[R(x,y,w,38,'chrome',{r:7,n:'Radio group / 종류'})]; const sw=w/opts.length; opts.forEach((o,i)=>{ if(i===act) a.push(R(x+i*sw+2,y+2,sw-4,34,'accent',{r:6})); a.push(Tx(x+i*sw+12,y+9,o,{z:13,b:500,c:i===act?'onAccent':'text'}));}); return a;};
const cap=(x,y,t)=>Tx(x,y,t,{z:11,b:600,c:'muted',n:'Note / '+t});
const SPEC=th=>{const X=892, L=272, W=572; return [...shell('새 분석','분석 범위 확인','포함할 위치와 제외 규칙을 확인한 뒤 시작합니다.',th),
 card(248,174,620,700),Tx(L,198,'포함할 위치',{z:18,b:600}),...thead(L,236,W,[[12,'경로'],[428,'확인']]),...row(L,272,W,[[12,'C:\\Users\\민서'],[428,'접근 가능']],47),
 Tx(L,344,'−  제외 규칙',{z:18,b:600}),Tx(L+118,349,'2개',{z:13,c:'muted'}),
 Tx(L,378,'넣은 폴더나 이름에 맞는 것은 분석하지 않습니다. 크기와 항목 수에도 들어가지 않습니다.',{z:12,c:'muted'}),
 ...thead(L,404,W,[[12,'종류'],[130,'값']]),
 ...row(L,440,W,[[12,'이름 패턴'],[130,'*.tmp']]),...btnS(L+W-132,446,58,'고치기'),...btnS(L+W-66,446,58,'지우기'),
 ...row(L,481,W,[[12,'경로'],[130,'C:\\Users\\민서\\비공개']]),...btnS(L+W-132,487,58,'고치기'),...btnS(L+W-66,487,58,'지우기'),
 ...seg(L,544,176,['경로','이름 패턴'],1),R(L+184,544,300,38,'surface',{r:7,s:'accent',sw:2,n:'Field / 제외할 이름 패턴 (초점)'}),Tx(L+196,553,'*.bak',{z:13}),...btn(L+492,544,80,'추가','primary'),
 Tx(L,594,'이름이 맞는 파일과 폴더를 어디에 있든 분석하지 않습니다.',{z:12,c:'muted'}),Tx(L,612,'*는 글자 여러 개, ?는 글자 하나입니다. 예: *.tmp, node_modules',{z:12,c:'muted'}),Tx(L+478,612,'+ 패턴 쓰는 법',{z:12,b:600,c:'accent'}),
 R(L,650,W,1,'line'),Tx(L,668,'볼륨 경계   ● 현재 볼륨만   ○ 지정한 하위 마운트 포함',{z:13}),
 Tx(L,790,'포함 위치 1개  ·  제외 규칙 2개',{z:13,c:'muted'}),...btn(L,816,110,'선택 취소'),...btn(L+122,816,140,'분석 범위 검토'),...btn(L+274,816,298,'분석 시작','primary'),
 cap(X,174,'접힌 모습 · 규칙이 없으면 접힘 / 규칙이 있으면 펼쳐서 엶(되살린 범위 포함)'),
 card(X,196,512,40,{s:'line',n:'Collapsed / none'}),Tx(X+16,207,'+  제외 규칙',{z:14,b:600}),Tx(X+118,208,'없음',{z:13,c:'muted'}),
 card(X,242,512,40,{s:'line',n:'Collapsed / summary'}),Tx(X+16,253,'+  제외 규칙',{z:14,b:600}),Tx(X+118,254,'2개 · *.tmp, C:\\Users\\민서\\비공개',{z:13,c:'muted'}),
 cap(X,302,'종류 「경로」 · 직접 입력 + 폴더 선택 대화상자(0.1 포함) · 값이 비면 「추가」는 비활성'),
 ...seg(X,324,160,['경로','이름 패턴'],0),R(X+168,324,168,38,'surface',{r:7,s:'line',n:'Field / 제외할 경로'}),Tx(X+180,333,'C:\\Folder\\Subfolder',{z:13,c:'muted'}),...btn(X+344,324,96,'폴더 선택…'),...btn(X+448,324,64,'추가','disabled'),
 Tx(X,370,'이 폴더와 그 안의 모든 것을 분석하지 않습니다. 파일 하나의 경로도 넣을 수 있습니다.',{z:12,c:'muted',w:512}),
 cap(X,418,'칸의 오류 · 거부된 규칙은 목록에 들어가지 않고 칸에 남아 이유를 봄'),
 ...seg(X,440,160,['경로','이름 패턴'],1),R(X+168,440,272,38,'surface',{r:7,s:'err',sw:2,n:'Field / invalid'}),Tx(X+180,449,'src/*.tmp',{z:13}),...btn(X+448,440,64,'추가'),
 Tx(X,486,'✕  이름 패턴에는 \\ 나 / 를 넣을 수 없습니다. 특정 위치를 빼려면 종류를 「경로」로 바꾸세요.',{z:12,c:'err',w:512}),
 cap(X,534,'행의 상태 · 「상태」 열 대신 문제가 있는 행의 둘째 줄(색 + 아이콘 + 글)'),
 R(X,556,512,58,'warnBg',{r:6,n:'Row / 효과 없음'}),Tx(X+12,565,'경로',{z:13}),Tx(X+100,565,'D:\\백업',{z:13}),Tx(X+100,587,'!  포함한 폴더 밖이라 제외되는 것이 없습니다',{z:12,b:500,c:'warn'}),...btnS(X+512-70,571,58,'지우기'),
 R(X,618,512,74,'sel',{r:6,n:'Row / 확인 못 함'}),Tx(X+12,627,'이름 패턴',{z:13}),Tx(X+100,627,'Thumbs.db',{z:13}),Tx(X+100,649,'ⓘ  이 드라이브에서는 경로를 정확히 비교할 수 없습니다. 대소문자나 글자 표기가 다르면 빼려던 것이 분석에 들어올 수 있습니다.',{z:12,b:500,c:'accent',w:330}),...btnS(X+512-70,641,58,'지우기'),
 R(X,696,512,58,'errBg',{r:6,n:'Row / 쓸 수 없음'}),R(X,696,3,58,'err'),Tx(X+12,705,'경로',{z:13}),Tx(X+100,705,'..\\임시',{z:13}),Tx(X+100,727,'✕  이 규칙은 쓸 수 없어 적용되지 않습니다. 고치거나 지워 주세요',{z:12,b:500,c:'err'}),
 R(X,758,512,44,'sel',{r:6,n:'Row / 고치는 중'}),Tx(X+12,771,'이름 패턴',{z:13}),Tx(X+100,771,'*.tmp',{z:13}),Tx(X+420,772,'고치는 중',{z:12,b:600,c:'accent'}),
 cap(X,822,'값 칸에 추가하지 않은 글이 있으면 「분석 시작」은 시작하지 않고 칸으로 초점을 옮김'),
 Tx(X,844,'✕  추가하지 않은 제외 규칙이 있습니다. 「추가」를 누르거나 칸을 비운 뒤 시작해 주세요.',{z:12,c:'err',w:512})];};


const PAL={light:{bg:'#f5f5f6',text:'#24262b',muted:'#656a73',sel:'#e4eaf5',accent:'#315eba',onAccent:'#ffffff',surface:'#ffffff',line:'#dadce0',warnBg:'#fbf1dc',warn:'#7a5200',errBg:'#fbe6e4',err:'#a52a22',okBg:'#e3f2e8',ok:'#1f6b3a',chrome:'#ecedef'},
 dark:{bg:'#1f1f1f',text:'#e6e6e6',muted:'#a6a9af',sel:'#293749',accent:'#78adfa',onAccent:'#101923',surface:'#252526',line:'#3c3c3c',warnBg:'#3a2f17',warn:'#f0c36a',errBg:'#3d201e',err:'#f29b93',okBg:'#1d3326',ok:'#8fd3a5',chrome:'#181818'}};
const root=penpot.currentPage.root; const NAME='scope · 제외 규칙 입력 (F01 · 디자인팀 설계)';
const ref=root.children.find(c=>c.id==='bbd3d9fd-83b3-8030-8008-f1a8ba901c32'); const fonts={};
for(const c of ref.children){ if(c.type==='text'&&!fonts[c.fontWeight]) fonts[c.fontWeight]={fontId:c.fontId,fontFamily:c.fontFamily,fontVariantId:c.fontVariantId,fontStyle:c.fontStyle}; }
const want=(globalThis.__which||['light','dark']); const made=[];
for(const th of want){
 const by=th==='light'?0:1040, bx=19760;
 for(const old of root.children.filter(c=>c.type==='board'&&c.name===NAME&&Math.round(c.y)===by)) old.remove();
 const P=PAL[th]; const board=penpot.createBoard(); board.name=NAME; board.resize(1440,960); board.x=bx; board.y=by; board.fills=[{fillColor:P.bg,fillOpacity:1}];
 let n=0;
 for(const it of SPEC(th)){
  if(it.k==='r'){ const r=penpot.createRectangle(); r.name=it.n||'Surface'; r.resize(it.w,it.h); r.fills=it.f?[{fillColor:P[it.f],fillOpacity:it.o==null?1:it.o}]:[]; if(it.r) r.borderRadius=it.r; if(it.s) r.strokes=[{strokeColor:P[it.s],strokeWidth:it.sw||1,strokeStyle:'solid',strokeAlignment:'inner',strokeOpacity:1}]; board.appendChild(r); r.x=bx+it.x; r.y=by+it.y; }
  else { const t=penpot.createText(it.t); t.name=(it.n||it.t).slice(0,60); const w=String(it.b||400); const f=fonts[w]||fonts['400']; t.fontId=f.fontId; t.fontFamily=f.fontFamily; t.fontVariantId=f.fontVariantId; t.fontWeight=w; t.fontSize=String(it.z||13); t.lineHeight='1.2'; t.fills=[{fillColor:P[it.c||'text'],fillOpacity:1}];
   if(it.w){ t.growType='auto-height'; t.resize(it.w,t.height); } else t.growType='auto-width';
   board.appendChild(t); t.x=bx+it.x; t.y=by+it.y; }
  n++; }
 made.push({th,id:board.id,n});
}
return {made,fonts:Object.keys(fonts)};
