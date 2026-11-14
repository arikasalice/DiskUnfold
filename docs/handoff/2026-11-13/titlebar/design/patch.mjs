
import { readFileSync, writeFileSync } from "node:fs";
const f = "D:/Github/DiskUnfold/docs/handoff/2026-11-13/titlebar/design/build-frames.mjs";
let s = readFileSync(f, "utf8");
const start = s.indexOf("function frame(platform, theme) {");
const end = s.indexOf("const names = [];");
const body = `function frame(platform, theme) {
  const t = T[theme];
  const A = 232; // product composition height; annotations live below it
  let h, bar = "", rules = [];
  if (platform === "macos") {
    h = 44;
    bar += '<circle cx="20" cy="22" r="6" fill="#ff5f57"/><circle cx="40" cy="22" r="6" fill="#febc2e"/><circle cx="60" cy="22" r="6" fill="#28c840"/>';
    bar += brand(78, 22, t, "시작 점검");
    rules = ["높이 44 · 배경 = 페이지 배경(투명 제목 영역) · 아래 1px 구분선", "왼쪽 78px는 신호등 예약 영역, 오른쪽 여백 12px", "전체 화면에서는 신호등이 사라지므로 왼쪽 여백 12px", "빈 영역 전체가 창 끌기 영역, 더블클릭은 OS 확대 동작"];
  } else if (platform === "windows") {
    h = 40;
    bar += brand(12, 20, t, "시작 점검");
    const bx = W - 138;
    bar += '<path d="M' + (bx + 18) + ' 20h10" stroke="' + t.text + '" stroke-width="1"/>';
    bar += '<rect x="' + (bx + 46 + 18) + '" y="15" width="10" height="10" fill="none" stroke="' + t.text + '" stroke-width="1"/>';
    bar += '<path d="M' + (bx + 92 + 18) + " 15l10 10M" + (bx + 92 + 28) + ' 15l-10 10" stroke="' + t.text + '" stroke-width="1"/>';
    rules = ["높이 40 · 배경 = 페이지 배경 · 아래 1px 구분선", "왼쪽 여백 12px, 오른쪽 138px(46×3)는 기본 캡션 버튼 예약 영역", "캡션 버튼은 OS가 그리며 앱은 그 영역에 아무것도 두지 않음", "빈 영역 전체가 창 끌기 영역, 최대화 시에도 같은 규칙"];
  } else {
    h = 46;
    bar += '<rect x="0" y="0" width="' + W + '" height="46" fill="' + t.surface + '"/>';
    bar += txt(W / 2, 21, "DiskUnfold", 13, t.text, 700, "middle") + txt(W / 2, 37, "시작 점검", 11, t.muted, 400, "middle");
    for (const [i, g] of [[0, "M-4 0h8"], [1, "M-4 -4h8v8h-8z"], [2, "M-4 -4l8 8M4 -4l-8 8"]]) {
      const cx = W - 22 - (2 - i) * 34;
      bar += '<circle cx="' + cx + '" cy="23" r="12" fill="' + t.hover + '"/><path transform="translate(' + cx + ' 23)" d="' + g + '" stroke="' + t.text + '" stroke-width="1.2" fill="none"/>';
    }
    rules = ["네이티브 GtkHeaderBar 사용 · 높이와 버튼 모양은 GTK 테마를 따름", "제목 = DiskUnfold, 부제 = 현재 위치 · 로고는 넣지 않음", "버튼 위치는 데스크톱 설정(gtk-decoration-layout)을 따름", "웹 화면 안에는 상단 바를 그리지 않음 (--du-titlebar-height: 0)"];
  }
  const label = { macos: "macOS · 투명 제목 영역 + 앱 상단 바", windows: "Windows · WebView2 상단 바 + 기본 캡션 버튼", linux: "Linux · GTK4 헤더바" }[platform];
  let s = '<svg xmlns="http://www.w3.org/2000/svg" width="' + W + '" height="' + H + '" viewBox="0 0 ' + W + " " + H + '">';
  s += '<rect width="' + W + '" height="' + H + '" fill="' + t.page + '"/>';
  s += bar + '<path d="M0 ' + (h - 0.5) + "H" + W + '" stroke="' + t.border + '" stroke-width="1"/>';
  s += content(h, t, "시작 상태와 개인정보 보호");
  // measurement guides sit in the right gutter of the product area
  s += dimV(W - 330, 0, h, h + " 상단 바", t.note) + dimV(W - 250, h, h + 24, "24 본문 상단 여백", t.note);
  // annotation strip
  s += '<rect x="0" y="' + A + '" width="' + W + '" height="' + (H - A) + '" fill="' + t.surface + '"/><path d="M0 ' + (A + 0.5) + "H" + W + '" stroke="' + t.border + '"/>';
  s += txt(24, A + 24, label + " · " + (theme === "light" ? "Light" : "Dark"), 12, t.text, 700);
  rules.forEach((r, i) => { s += txt(24, A + 46 + i * 17, "· " + r, 11, t.muted, 400); });
  s += "</svg>";
  const name = "titlebar-" + platform + "-" + theme;
  writeFileSync(out + "/" + name + ".svg", s);
  return name;
}
`;
s = s.slice(0, start) + body + s.slice(end);
s = s.replace("const W = 960, H = 340;", "const W = 960, H = 350;");
s = s.replace('height="340">', 'height="350">');
// shorter content block so it fits the product area
s = s.replace('width="430" height="120"', 'width="430" height="64"').replace('width="442" height="120"', 'width="442" height="64"');
writeFileSync(f, s);
