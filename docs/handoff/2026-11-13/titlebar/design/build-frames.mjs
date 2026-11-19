
import { writeFileSync, mkdirSync, readFileSync } from "node:fs";
const out = "D:/Github/DiskUnfold/docs/handoff/2026-11-13/titlebar/design";
mkdirSync(out, { recursive: true });
const icon = "data:image/svg+xml;base64," + readFileSync("D:/Github/DiskUnfold/gui-web/src/app/assets/diskunfold-icon.svg").toString("base64");
const T = {
  light: { page: "#f5f5f6", surface: "#ffffff", text: "#242528", muted: "#626771", border: "#d9dce1", accent: "#2f5fc5", accentText: "#ffffff", note: "#8a3ffc", hover: "#e5e7ea" },
  dark: { page: "#18191c", surface: "#202226", text: "#f1f2f4", muted: "#b3b6bd", border: "#41454d", accent: "#94b5ff", accentText: "#17233f", note: "#c9a6ff", hover: "#363a41" },
};
const F = 'font-family="Segoe UI, system-ui, sans-serif"';
const W = 960, H = 350;
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const txt = (x, y, s, size, fill, weight = 400, anchor = "start") => '<text x="' + x + '" y="' + y + '" ' + F + ' font-size="' + size + '" font-weight="' + weight + '" fill="' + fill + '" text-anchor="' + anchor + '">' + esc(s) + "</text>";
// dimension marker: horizontal span
const dimH = (x1, x2, y, label, c) => '<path d="M' + x1 + " " + y + "H" + x2 + "M" + x1 + " " + (y - 4) + "v8M" + x2 + " " + (y - 4) + 'v8" stroke="' + c + '" stroke-width="1" fill="none"/>' + txt((x1 + x2) / 2, y + 14, label, 10, c, 600, "middle");
const dimV = (x, y1, y2, label, c) => '<path d="M' + x + " " + y1 + "V" + y2 + "M" + (x - 4) + " " + y1 + "h8M" + (x - 4) + " " + y2 + 'h8" stroke="' + c + '" stroke-width="1" fill="none"/>' + txt(x + 8, (y1 + y2) / 2 + 4, label, 10, c, 600);

function brand(x, cy, t, location) {
  let s = '<image href="' + icon + '" x="' + x + '" y="' + (cy - 10) + '" width="20" height="20"/>';
  s += txt(x + 28, cy + 4.5, "DiskUnfold", 13, t.text, 650);
  if (location) s += txt(x + 104, cy + 4.5, "/", 13, t.border, 400) + txt(x + 116, cy + 4.5, location, 13, t.muted, 400);
  return s;
}
function content(top, t, lang) {
  const y = top + 24;
  return txt(36, y + 22, lang, 22, t.text, 660) + txt(36, y + 50, "이 Mac의 시작 점검과 전체 디스크 접근 권한을 확인합니다.", 14, t.muted) +
    '<rect x="36" y="' + (y + 70) + '" width="430" height="64" rx="10" fill="' + t.surface + '" stroke="' + t.border + '"/>' +
    '<rect x="482" y="' + (y + 70) + '" width="442" height="64" rx="10" fill="' + t.surface + '" stroke="' + t.border + '"/>';
}
function frame(platform, theme) {
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
const names = [];
for (const p of ["macos", "windows", "linux"]) for (const th of ["light", "dark"]) names.push(frame(p, th));
writeFileSync(out + "/frames.json", JSON.stringify(names));
writeFileSync(out + "/index.html", '<!doctype html><meta charset="utf-8"><body style="margin:0;background:#888;display:grid;gap:12px;padding:12px;grid-template-columns:960px 960px">' + names.map((n) => '<img src="' + n + '.svg" width="960" height="350">').join("") + "</body>");
console.log(names.join(","));
