
const {R,Tx,tw,shell,btn,card,badge,band,thead,row,seg,chk,cap}=B;
const wrap=(name,fn)=>{const f0=SPEC[name]; if(!f0) throw new Error('missing '+name); SPEC[name]=th=>fn(f0(th),th);};
const K=Object.keys(SPEC); const find=p=>K.find(k=>k.startsWith(p));
// 1. treemap text on pale cells
const tmFix=a=>{const cells=a.filter(i=>i.k==='r'&&['tm3','tm4','tm5'].includes(i.f)); for(const t of a){ if(t.k==='t'&&t.c==='tmText'&&cells.some(r=>t.x>=r.x&&t.x<r.x+r.w&&t.y>=r.y&&t.y<r.y+r.h)) t.c='tmTextB'; } return a;};
wrap(find('sample · 체험 중'),tmFix); wrap(find('scan ·'),tmFix);
// 2. dialog
SPEC[find('sample · 체험 종료')]=th=>[...shell('공간 탐색',null,null,th),...band(212,40,1228,'info','샘플 체험 · 실제 파일과 분리됨 · 변경 실행 불가',52),Tx(248,112,'내 디스크의 공간',{z:28,b:700}),
 R(0,0,1440,960,'scrim',{o:0.45,n:'Scrim'}),card(450,340,540,232,{n:'Dialog'}),Tx(482,372,'샘플 체험을 끝낼까요?',{z:20,b:700}),Tx(482,418,'체험 중 고른 항목과 보기 설정은 저장되지 않고 사라집니다.\n실제 파일은 바뀌지 않습니다.',{z:14,c:'muted',w:476}),
 ...btn(660,502,140,'계속 체험','focus'),...btn(812,502,146,'체험 끝내기','primary'),cap(450,588,'처음 초점: 계속 체험 · Esc: 계속 체험 · 닫으면 초점은 “샘플 닫기”로')];
// 3. groups result
wrap(find('groups · 결과'),a=>{ a=a.filter(i=>!((i.k==='r'&&i.n==='Button / 탐색으로 돌아가기'&&i.y===820)||(i.k==='t'&&i.t==='탐색으로 돌아가기'&&i.y===829))); for(const i of a){ if(i.k==='r'&&i.n==='Badge / 가상'&&i.y===375) i.f='surface'; } return a;});
// 4. scope
wrap(find('scope ·'),a=>{ for(const i of a){ if(i.k==='t'&&i.t.startsWith('C: 드라이브에서')) i.t='C: 드라이브에서 사용할 수 있습니다. 관리자 권한 창이 뜹니다.\n허용하지 않아도 기본 방식으로 분석을 계속합니다.'; if(i.k==='t'&&i.t.startsWith('분석 시작은')) i.t=i.n='분석 시작은 수집 조건보다 위에 있어 창 높이 720px에서도 보임 · 아래는 수집 조건의 처음 상태(접힘)'; }
 a.push(card(248,790,620,84,{s:'line',n:'Collapsed state'}),Tx(272,806,'수집 조건',{z:15,b:600}),Tx(272,834,'자원 사용 자동 · 추가 정보 없음',{z:13,c:'muted'}),Tx(716,820,'수집 조건 바꾸기 ▾',{z:13,b:600,c:'accent'})); return a;});
// 5. browse
SPEC[find('browse ·')]=th=>[...shell('공간 탐색','C:\\Users\\민서','분석 중 · 수집한 결과 일부를 보고 있습니다.',th),
 card(248,174,1156,56),Tx(272,192,'부분 결과  ·  확인한 항목 128,420개  ·  새 결과를 자동으로 반영하고 있습니다',{z:14}),...badge(1244,190,'새 결과 12건 · 반영','info'),
 ...thead(248,250,1156,[[24,'이름'],[452,'크기'],[612,'수집 상태']]),
 ...row(248,286,1156,[[24,'문서'],[452,'18.2 GB'],[612,'수집 완료']]),...row(248,334,1156,[[24,'영상'],[452,'4.1 GB 이상'],[612,'먼저 분석하는 중 · 남은 폴더 42']]),
 ...row(248,382,1156,[[24,'사진'],[452,'확인 중',{c:'muted'}],[612,'먼저 분석 요청됨',{b:600,c:'accent'}]]),
 ...row(248,430,1156,[[24,'다운로드'],[452,'확인 중',{c:'muted'}],[612,'아직 수집하지 않음']],true),R(248,430,1156,47,null,{s:'accent',sw:2,n:'Focus ring'}),...btn(1280,434,104,'먼저 분석'),
 ...row(248,478,1156,[[24,'AppData'],[452,'확인 불가',{c:'muted'}],[612,'일부 읽지 못함']]),...row(248,526,1156,[[24,'메모.txt'],[452,'2 KB'],[612,'수집 완료']]),
 card(1020,482,240,152,{s:'line',n:'ContextMenu'}),Tx(1036,496,'열기',{z:13}),R(1024,524,232,34,'sel',{r:5}),Tx(1036,531,'먼저 분석',{z:13,b:600}),Tx(1036,568,'경로 복사',{z:13}),Tx(1036,602,'파일 탐색기에서 보기',{z:13}),
 card(1024,820,380,52,{s:'line',n:'Toast'}),Tx(1044,836,'✓  ‘사진’을 먼저 분석합니다',{z:13,b:500}),
 cap(248,660,'사진: 요청한 뒤의 행 (상태 문구가 바뀌고 단추는 사라짐) · 다운로드: 포인터나 초점이 간 행 (단추가 보임, 오른쪽 클릭 메뉴에도 같은 동작)'),
 cap(248,682,'수집이 끝난 폴더와 파일에는 먼저 분석을 보이지 않음'),
 cap(248,704,'미정 · 사용자 결정 대기: 새 결과 자동 반영(이 그림) / 발행 설계 FR3.HD1.gui-web.DD1의 “사용자가 갱신을 고를 때 반영”. 결정 전에는 발행 설계를 따름')];
// 6. settings language
wrap(find('settings · 모양과'),a=>{ for(const i of a){ if(i.k==='r'&&i.y===174&&i.h===700) i.h=716; if(i.k==='r'&&i.y===820) i.y=836; if(i.k==='t'&&i.y===829) i.y=845; } return a;});
// 7. context menu height
wrap(find('components ·'),a=>{ for(const i of a){ if(i.k==='r'&&i.x===1000&&i.y===474&&i.h===170) i.h=152; } return a;});
globalThis.SPEC_FIXED=true;
