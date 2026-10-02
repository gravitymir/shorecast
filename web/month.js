// Month calendar: six Monday-first weeks, every day links to its page.

import { clockIn, esc, getJson, isoDate, monthTitle, todayIn, weatherInfo } from "./common.js";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

async function main() {
  let today;
  let tz = Intl.DateTimeFormat().resolvedOptions().timeZone;
  try {
    const site = await getJson("/api/site");
    tz = site.tz;
    today = todayIn(site.tz, new Date(site.now));
  } catch {
    today = todayIn(tz);
  }

  // /month/2026/10, or the current month on "/".
  const m = /^\/month\/(\d{1,4})\/(\d{1,2})$/.exec(location.pathname);
  const [ty, tm] = today.split("-").map(Number);
  const year = m ? Number(m[1]) : ty;
  const month = m ? Number(m[2]) : tm;

  const title = monthTitle(year, month);
  document.title = `${title} · ShoreCast`;
  const prev = new Date(Date.UTC(year, month - 2, 1));
  const next = new Date(Date.UTC(year, month, 1));
  const href = (d) => `/month/${d.getUTCFullYear()}/${d.getUTCMonth() + 1}`;

  // Back up from the 1st to the Monday on or before it.
  const lead = (new Date(Date.UTC(year, month - 1, 1)).getUTCDay() + 6) % 7;
  let cells = "";
  for (let i = 0; i < 42; i++) {
    const iso = isoDate(year, month, 1 - lead + i);
    const [, cm, cd] = iso.split("-").map(Number);
    const cls = ["cell", cm !== month && "out", iso === today && "today"].filter(Boolean).join(" ");
    cells += `<a class="${cls}" href="/day/${iso}" data-date="${iso}"><span class="num">${cd}</span></a>`;
  }

  document.getElementById("app").innerHTML = `
    <nav class="bar">
      <div class="left"><a class="btn" href="${href(prev)}">&larr; Prev</a><a class="btn" href="/">Today</a></div>
      <h1>${title}</h1>
      <div class="right"><a class="btn" href="${href(next)}">Next &rarr;</a></div>
    </nav>
    <div class="grid">
      ${WEEKDAYS.map((d) => `<div class="dow">${d}</div>`).join("")}
      ${cells}
    </div>`;

  // Fill the cells in once the day summaries arrive.
  const first = isoDate(year, month, 1 - lead);
  const last = isoDate(year, month, 1 - lead + 41);
  try {
    const days = await getJson(`/api/summary?from=${first}&to=${last}`);
    const hm = clockIn(tz);
    for (const d of days) {
      const cell = document.querySelector(`.cell[data-date="${d.date}"]`);
      if (cell) cell.insertAdjacentHTML("beforeend", summaryHtml(d, hm));
    }
  } catch (err) {
    console.warn("no day summaries:", err);
  }
}

/** The small print inside a day cell. */
function summaryHtml(d, hm) {
  const lines = [];
  if (d.weather != null) {
    const [icon, name] = weatherInfo(d.weather);
    lines.push(`<span class="wx" title="${name}">${icon}</span>`);
  }
  // Cruise liners (gold) and every other ship; others are left out where
  // the port has not published its schedule yet (about a week ahead).
  const ships = [];
  if (d.cruises) {
    ships.push(`<span class="liners" title="Cruise liners calling"><span class="icon">🛳️</span><b>${d.cruises}</b></span>`);
  }
  if (d.others != null) {
    ships.push(`<span title="Other ships in and out"><span class="icon">⛵</span><b>${d.others}</b></span>`);
  }
  if (ships.length) lines.push(`<div class="ships-count">${ships.join("")}</div>`);
  if (d.racing.length) {
    const more = d.racing.length > 1 ? ` +${d.racing.length - 1}` : "";
    const title = esc(d.racing.join(" · "));
    lines.push(`<div class="racing" title="Yacht racing: ${title}"><span class="icon">🏁</span>${esc(d.racing[0])}${more}</div>`);
  }
  lines.push(`<div title="Sunrise / sunset"><span class="icon">☀️</span>${hm(d.sunrise)}–${hm(d.sunset)}</div>`);
  lines.push(`<div title="Moonrise / moonset"><span class="icon">🌙</span>↑${hm(d.moonrise)} ↓${hm(d.moonset)}</div>`);
  if (d.lows.length) {
    lines.push(`<div title="Low water"><span class="icon">🌊</span>${d.lows.map(([t]) => hm(t)).join(" · ")}</div>`);
  }
  return `<div class="summary">${lines.join("")}</div>`;
}

main();
