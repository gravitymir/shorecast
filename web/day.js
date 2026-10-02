// Day page: 24-hour timelines for the Sun, Moon and tides, drawn from
// /api/day/YYYY-MM-DD.

import { addDays, esc, getJson, pad, parseIsoDate, weatherInfo } from "./common.js";

async function main() {
  const app = document.getElementById("app");
  const date = decodeURIComponent(location.pathname.split("/").pop());
  const ymd = parseIsoDate(date);
  if (!ymd) {
    app.innerHTML = `<p class="soon">Unknown date ${esc(date)}.</p>`;
    return;
  }

  let data;
  try {
    data = await getJson(`/api/day/${date}`);
  } catch (err) {
    app.innerHTML = `<p class="soon">Could not load the day: ${esc(err.message)}</p>`;
    return;
  }

  const [year, month, day] = ymd;
  const noon = new Date(Date.UTC(year, month - 1, day, 12));
  const title = noon.toLocaleDateString("en-GB", {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
  const monthName = noon.toLocaleDateString("en-GB", { month: "long", timeZone: "UTC" });
  document.title = `${title} · ShoreCast`;

  const view = new View(data);
  const { site } = data;
  app.innerHTML = `
    <nav class="bar"><div class="left">
      <a class="btn" href="/month/${year}/${month}">&larr; ${monthName}</a>
      <a class="btn" href="/day/${addDays(date, -1)}">&lsaquo; Prev day</a>
      <a class="btn" href="/day/${addDays(date, 1)}">Next day &rsaquo;</a>
    </div></nav>
    <div class="titlebar"><h1>${title}</h1><div class="clock" title="Time at ${esc(site.name)}"></div></div>
    <p class="sub">${date} · ${esc(site.name)} · ${site.lat.toFixed(4)}, ${site.lon.toFixed(4)} · times ${esc(site.tz)}</p>
    ${view.sun()}
    ${view.moon()}
    ${view.weather()}
    ${view.tides()}
    ${view.ships()}
    ${view.sailing()}
    ${placeholder("Plan")}`;
  layoutShips(app);
  addEventListener("resize", () => layoutShips(app));
  startClock(app, view);
  weatherTips(app, view);
}

const COMPASS = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];

function compass(degrees) {
  return COMPASS[Math.round(degrees / 22.5) % 16];
}

/**
 * Our own tooltips, instead of the browser's small grey ones. Hovering
 * (or tapping) an element with `data-<key>="i"` inside `area` highlights
 * every element with that index and shows `html[i]` above the pointer.
 */
function tooltips(area, key, html, extraClass = "") {
  if (!area) return;
  const tip = document.createElement("div");
  tip.className = `tip ${extraClass}`;
  tip.hidden = true;
  document.body.append(tip);

  let shown = null;
  const hide = () => {
    tip.hidden = true;
    area.querySelectorAll(".hover").forEach((el) => el.classList.remove("hover"));
    shown = null;
  };
  // Centred above the pointer, kept inside the window.
  const place = (e) => {
    const w = tip.offsetWidth;
    const x = Math.max(8, Math.min(innerWidth - w - 8, e.clientX - w / 2));
    const y = Math.max(8, e.clientY - tip.offsetHeight - 16);
    tip.style.left = `${x + scrollX}px`;
    tip.style.top = `${y + scrollY}px`;
  };
  area.addEventListener("mouseover", (e) => {
    const el = e.target.closest(`[data-${key}]`);
    if (!el) return hide();
    const i = el.dataset[key];
    if (i !== shown) {
      hide();
      shown = i;
      area.querySelectorAll(`[data-${key}="${i}"]`).forEach((c) => c.classList.add("hover"));
      tip.innerHTML = html[i];
      tip.hidden = false;
    }
    place(e);
  });
  area.addEventListener("mousemove", (e) => {
    if (shown != null) place(e);
  });
  area.addEventListener("mouseleave", hide);
}

/** Hover cards for the weather columns and the ship labels. */
function weatherTips(root, view) {
  tooltips(root.querySelector(".wx-cols"), "wx", view.weatherTips);
  tooltips(root.querySelector(".track.ships"), "ship", view.shipTips, "big");
  tooltips(root.querySelector(".track.sailing"), "sail", view.sailTips, "big");
}

/** Ticks the clock and slides the red "now" lines, once a second. */
function startClock(root, view) {
  // Follow the server's clock, not the viewer's, in case they disagree.
  const offset = view.now - Date.now();
  const clock = root.querySelector(".clock");
  const fmt = new Intl.DateTimeFormat("en-GB", {
    timeZone: view.data.site.tz,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
  });
  const tick = () => {
    const now = Date.now() + offset;
    clock.textContent = fmt.format(new Date(now));
    const today = view.start <= now && now < view.end;
    for (const line of root.querySelectorAll(".now")) {
      line.hidden = !today;
      line.style.left = `${view.pct(now)}%`;
    }
  };
  tick();
  setInterval(tick, 1000);
}

const SHIP_ICONS = {
  cruise: "🛳️",
  ferry: "⛴️",
  container: "📦",
  tanker: "🛢️",
  bulk: "🚢",
  cargo: "🚢",
  navy: "⚓",
  fishing: "🎣",
  other: "🚤",
};

const SAIL_ICONS = { racing: "🏁", training: "🎓", cruising: "🧭" };

/** Legend names; the colours are the .cat-* rules in style.css. */
const SHIP_KINDS = {
  cruise: "cruise",
  ferry: "ferry",
  container: "container",
  tanker: "tanker",
  cargo: "cargo / bulk",
  fishing: "fishing",
  navy: "navy",
  other: "other",
};

/** Stacks ship labels in rows so they never overlap, then sizes the track. */
function layoutShips(root) {
  const LANE = 46;
  const GAP = 8;
  for (const track of root.querySelectorAll(".track.ships")) {
    const width = track.clientWidth;
    const laneEnds = [];
    const placed = [...track.querySelectorAll(".ship")].map((label) => {
      const pct = Number(label.dataset.pct);
      const w = label.offsetWidth;
      const x = Math.max(0, Math.min(width - w, (pct / 100) * width - w / 2));
      let lane = laneEnds.findIndex((end) => end + GAP <= x);
      if (lane < 0) lane = laneEnds.push(0) - 1;
      laneEnds[lane] = x + w;
      return { label, pct, x, lane };
    });
    track.style.height = `${Math.max(1, laneEnds.length) * LANE + 18}px`;
    for (const { label, pct, x, lane } of placed) {
      const top = 6 + lane * LANE;
      label.style.left = `${x}px`;
      label.style.top = `${top}px`;
      const tick = label.nextElementSibling;
      tick.style.left = `${pct}%`;
      tick.style.top = `${top + label.offsetHeight}px`;
    }
  }
}

function section(title, inner) {
  return `<section class="section"><h2>${title}</h2>${inner}</section>`;
}

/** A section whose title opens its facts line instead of a row of its own. */
function compactSection(inner) {
  return `<section class="section compact">${inner}</section>`;
}

function placeholder(title) {
  return section(title, `<p class="soon">Coming soon.</p>`);
}

function facts(items) {
  return `<ul class="facts">${items.map((i) => `<li>${i}</li>`).join("")}</ul>`;
}

class View {
  constructor(data) {
    this.data = data;
    this.start = Date.parse(data.start);
    this.end = Date.parse(data.end);
    this.now = Date.parse(data.site.now);
    this.clock = new Intl.DateTimeFormat("en-GB", {
      timeZone: data.site.tz,
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    });
  }

  /** Local "HH:MM" of an ISO time. */
  hm(iso) {
    return this.clock.format(new Date(iso));
  }

  /** Position of an ISO time along the day, in percent. */
  pct(iso) {
    const t = typeof iso === "number" ? iso : Date.parse(iso);
    return Math.min(100, Math.max(0, ((t - this.start) / (this.end - this.start)) * 100));
  }

  /**
   * A 24-hour track with `inner` drawn on it, "now" and an hour axis:
   * - `axis: false` when an hourly row below already shows the hours;
   * - `inside: spans` to write the hours on the track itself, dark over
   *   the spans (the sun or moon is up) and light elsewhere.
   */
  timeline(cls, inner, { axis = true, inside = null } = {}) {
    // The clock moves it every second and hides it on other days.
    const now = `<div class="now" title="Now" hidden></div>`;
    const labels = [];
    for (let h = 0; h <= 24; h += 3) {
      const pct = (h / 24) * 100;
      let cls = "";
      if (inside) {
        const t = this.start + (pct / 100) * (this.end - this.start);
        const lit = inside.some(([a, b]) => Date.parse(a) <= t && t <= Date.parse(b));
        cls = lit ? ' class="lit"' : "";
      }
      labels.push(`<span${cls} style="left:${pct}%">${pad(h)}</span>`);
    }
    if (inside) {
      const hours = `<div class="hours">${labels.join("")}</div>`;
      return `<div class="timeline"><div class="track ${cls}">${inner}${hours}${now}</div></div>`;
    }
    const ticks = axis ? `<div class="axis">${labels.join("")}</div>` : "";
    return `<div class="timeline"><div class="track ${cls}">${inner}${now}</div>${ticks}</div>`;
  }

  spans(up, cls) {
    return up
      .map(([from, to]) => {
        const left = this.pct(from);
        const width = this.pct(to) - left;
        return `<div class="span ${cls}" style="left:${left}%;width:${width}%" title="${this.hm(from)} – ${this.hm(to)}"></div>`;
      })
      .join("");
  }

  events(events, rise, set) {
    return events.map(
      (e) =>
        `${e.kind === "rise" ? rise : set} <b>${this.hm(e.time)}</b> · ${Math.round(e.azimuth)}° ${e.compass}`,
    );
  }

  sun() {
    const { sun } = this.data;
    // Like the moon: the title, then the icon opening the first fact.
    const items = this.events(sun.events, "Sunrise", "Sunset");
    if (items.length) items[0] = `☀️ ${items[0]}`;
    items.unshift(`<span class="tag">Sun</span>`);
    if (sun.up.length === 0) {
      items.push("The sun stays below the horizon");
    } else {
      const ms = sun.up.reduce((sum, [a, b]) => sum + Date.parse(b) - Date.parse(a), 0);
      const minutes = Math.round(ms / 60000);
      items.push(`Daylight <b>${Math.floor(minutes / 60)} h ${pad(minutes % 60)} min</b>`);
    }
    const track = this.timeline("sky", this.spans(sun.up, "sun"), { inside: sun.up });
    return compactSection(track + facts(items));
  }

  moon() {
    const { moon } = this.data;
    const items = [
      `<span class="tag">Moon</span>`,
      `${moon.emoji} ${moon.phase} · <b>${Math.round(moon.illuminated * 100)}%</b> lit`,
      ...this.events(moon.events, "Moonrise", "Moonset"),
    ];
    if (moon.events.length === 0) {
      items.push(`The moon stays ${moon.up.length ? "up" : "down"} all day`);
    }
    const track = this.timeline("sky", this.spans(moon.up, "moon"), { inside: moon.up });
    return compactSection(track + facts(items));
  }

  weather() {
    const hours = this.data.weather;
    if (hours.length === 0) {
      return section(
        "Weather",
        `<p class="soon">No forecast for this date (Open-Meteo forecasts 16 days ahead).</p>`,
      );
    }
    const HOUR = 3600000;
    // One tooltip per hour, shared by its block and its cell in the row.
    this.weatherTips = hours.map((h) => this.weatherTip(h));
    const blocks = hours
      .map((h, i) => {
        const [icon, , color] = weatherInfo(h.code, h.is_day);
        const left = this.pct(h.time);
        const width = this.pct(Date.parse(h.time) + HOUR) - left;
        return `<div class="wx-hour${h.is_day ? "" : " night"}" data-wx="${i}" style="left:${left}%;width:${width}%;background:${color}">${icon}</div>`;
      })
      .join("");

    const byHour = new Map(hours.map((h, i) => [Math.round((Date.parse(h.time) - this.start) / HOUR), i]));
    const row = [];
    for (let hour = 0; hour < 24; hour++) {
      const i = byHour.get(hour);
      const h = hours[i];
      const temp = h?.temperature != null ? `${Math.round(h.temperature)}°` : "–";
      const wind = h?.wind != null ? Math.round(h.wind) : "";
      const attr = i == null ? "" : ` data-wx="${i}"`;
      row.push(`<div${attr}><span>${pad(hour)}</span>${temp}<small>${wind}</small></div>`);
    }

    const rain = hours.reduce((sum, h) => sum + (h.precipitation ?? 0), 0);
    const temps = hours.map((h) => h.temperature).filter((t) => t != null);
    const gusts = Math.max(...hours.map((h) => h.gusts ?? 0));
    const vis = Math.min(...hours.map((h) => h.visibility ?? Infinity));
    const items = [
      temps.length && `Temperature <b>${Math.round(Math.min(...temps))}–${Math.round(Math.max(...temps))} °C</b>`,
      `Rain <b>${rain.toFixed(1)} mm</b>`,
      `Gusts up to <b>${Math.round(gusts)} km/h</b>`,
      Number.isFinite(vis) && `Visibility down to <b>${(vis / 1000).toFixed(0)} km</b>`,
    ].filter(Boolean);

    return section(
      "Weather",
      `<div class="wx-cols">` +
        this.timeline("weather", blocks, { axis: false }) +
        `<div class="hourly wx-row">${row.join("")}</div></div>` +
        facts(items) +
        `<p class="source">Open-Meteo forecast · columns: hour, °C, wind km/h · hover a column for details</p>`,
    );
  }

  /** The tooltip for one ship passage. */
  shipTip(p) {
    const icon = SHIP_ICONS[p.category] ?? SHIP_ICONS.other;
    const inbound = p.direction === "in";
    const row = (label, value) => (value ? `<dt>${label}</dt><dd>${esc(value)}</dd>` : "");
    let when = `${inbound ? "In" : "Out"} at ${this.hm(p.time)}`;
    if (p.actual && p.time !== p.scheduled) when += ` (planned ${this.hm(p.scheduled)})`;
    const meaning = inbound
      ? "pilot boards off the harbour mouth"
      : p.source === "cruise"
        ? "leaves the berth"
        : "leaves the berth; passes the point later";
    const unique = p.category === "cruise" ? ' class="unique"' : "";
    return `<b${unique}>${icon} ${esc(p.vessel)}</b><dl>${[
      row("Type", p.vessel_type.toLowerCase()),
      row("Length", p.length_m ? `${Math.round(p.length_m)} m` : null),
      row("When", when),
      row("Time is", p.source === "cruise" ? "at the berth (cruise schedule)" : meaning),
      row(inbound ? "Coming from" : "Going to", p.port),
      row("Route", `${p.from} → ${p.to}`),
      row("Status", `${p.status.toLowerCase()}${p.actual ? " · actual time" : ""}`),
      row("Source", p.source === "cruise" ? "Port of Cork cruise schedule" : "Port of Cork shipping schedule"),
    ].join("")}</dl>`;
  }

  /** The tooltip for one hour of weather, every number with its name. */
  weatherTip(h) {
    const [icon, name] = weatherInfo(h.code, h.is_day);
    const until = this.hm(Date.parse(h.time) + 3600000);
    const row = (label, value) => (value == null ? "" : `<dt>${label}</dt><dd>${value}</dd>`);
    const wind =
      h.wind == null
        ? null
        : `${Math.round(h.wind)} km/h` +
          (h.wind_from != null ? ` from ${compass(h.wind_from)}` : "") +
          (h.gusts != null ? `, gusts ${Math.round(h.gusts)} km/h` : "");
    return `<b>${this.hm(h.time)}–${until} · ${icon} ${name}</b><dl>${[
      row("Temperature", h.temperature != null ? `${h.temperature.toFixed(1)} °C` : null),
      row("Wind", wind),
      row("Cloud", h.cloud != null ? `${Math.round(h.cloud)}% of the sky` : null),
      row("Rain", h.precipitation != null ? `${h.precipitation.toFixed(1)} mm` : null),
      row("Visibility", h.visibility != null ? `${(h.visibility / 1000).toFixed(1)} km` : null),
    ].join("")}</dl>`;
  }

  ships() {
    const { ships } = this.data;
    if (ships.length === 0) {
      return section("Ships", `<p class="soon">No ships scheduled in or out.</p>`);
    }
    this.shipTips = ships.map((p) => this.shipTip(p));
    const marks = ships
      .map((p, i) => {
        const icon = SHIP_ICONS[p.category] ?? SHIP_ICONS.other;
        const dir = p.direction === "in" ? "in" : "out";
        const length = p.length_m ? ` · ${Math.round(p.length_m)} m` : "";
        // Colour by kind of ship; the arrow under the label gives the
        // direction: ▲ up into the label = arriving, ▼ down = leaving.
        const cls = `${dir} cat-${p.category}`;
        const arrow = dir === "in" ? "▲" : "▼";
        return `<div class="ship ${cls}" data-pct="${this.pct(p.time)}" data-ship="${i}">
            <b><span class="icon">${icon}</span> ${esc(p.vessel)}</b><span>${dir} ${this.hm(p.time)}${length}</span>
          </div><div class="ship-tick ${cls}" data-ship="${i}"><span class="arrow">${arrow}</span></div>`;
      })
      .join("");

    const legend = Object.entries(SHIP_KINDS)
      .map(([cat, name]) => `<span class="cat-${cat}"><i></i>${SHIP_ICONS[cat]} ${name}</span>`)
      .join("");

    const rows = ships
      .map((p) => {
        const dir = p.direction === "in" ? "in" : "out";
        const icon = SHIP_ICONS[p.category] ?? SHIP_ICONS.other;
        const length = p.length_m ? `${Math.round(p.length_m)} m` : "";
        const where = p.direction === "in" ? `from ${p.port || "sea"}` : `to ${p.port || "sea"}`;
        const late = p.actual && p.time !== p.scheduled ? ` <i>(planned ${this.hm(p.scheduled)})</i>` : "";
        return `<tr class="${dir} cat-${p.category}">
          <td><b>${this.hm(p.time)}</b>${late}</td>
          <td class="dir">${dir === "in" ? "▲" : "▼"} ${dir}</td>
          <td class="name">${icon} ${esc(p.vessel)}</td>
          <td>${esc(p.vessel_type.toLowerCase())}</td>
          <td>${length}</td>
          <td>${esc(where)}</td>
          <td>${esc(p.from)} → ${esc(p.to)}</td>
        </tr>`;
      })
      .join("");

    return section(
      "Ships",
      this.timeline("ships", marks) +
        `<div class="shiplist"><table>${rows}</table></div>` +
        `<div class="legend">${legend}<span>▲ in · ▼ out</span></div>` +
        `<p class="source">Port of Cork shipping and cruise schedules · arrivals: pilot boarding off the harbour mouth · departures: leaving the berth</p>`,
    );
  }

  sailing() {
    const sessions = this.data.sailing;
    if (sessions.length === 0) {
      return section("Sailing", `<p class="soon">No yacht club racing or training listed.</p>`);
    }
    const ROW = 36;
    this.sailTips = sessions.map((s) => this.sailTip(s));
    const bars = sessions
      .map((s, i) => {
        const left = this.pct(s.start);
        const width = Math.max(this.pct(s.end) - left, 1);
        const when = s.all_day ? "all day" : `${this.hm(s.start)}–${this.hm(s.end)}`;
        return `<a class="sail ${s.kind}" href="${esc(s.url)}" target="_blank" rel="noopener" data-sail="${i}"
            style="top:${4 + i * ROW}px;left:${left}%;width:${width}%">
            <span class="icon">${SAIL_ICONS[s.kind]}</span> <b>${esc(s.title)}</b> <span>${when}</span></a>`;
      })
      .join("");
    const height = sessions.length * ROW + 4;
    return section(
      "Sailing",
      this.timeline("sailing", bars).replace('class="track sailing"', `class="track sailing" style="height:${height}px"`) +
        `<p class="source">Royal Cork Yacht Club calendar, Crosshaven · 🏁 racing · 🎓 training · 🧭 cruising · dates are reliable, times on the water often not given</p>`,
    );
  }

  /** The tooltip for one yacht club event. */
  sailTip(s) {
    const row = (label, value) => (value ? `<dt>${label}</dt><dd>${esc(value)}</dd>` : "");
    const when = s.all_day ? "all day (the club gives no times)" : `${this.hm(s.start)}–${this.hm(s.end)}`;
    const classes = s.categories.filter((c) => !["All", "General"].includes(c)).join(", ");
    return `<b>${SAIL_ICONS[s.kind]} ${esc(s.title)}</b><dl>${[
      row("Kind", s.kind),
      row("When", when),
      row("Boats", classes),
      row("Source", "Royal Cork Yacht Club · click for the club page"),
    ].join("")}</dl>`;
  }

  tides() {
    const { tides } = this.data;
    const station = esc(tides.station.replace(/_/g, " "));
    const source = (extra = "") =>
      `<p class="source">Station ${station} · metres above chart datum · Marine Institute${extra}</p>`;
    if (tides.error) {
      return section("Tides", `<p class="soon">Tide data unavailable: ${esc(tides.error)}</p>${source()}`);
    }
    if (tides.series.length === 0) {
      return section("Tides", `<p class="soon">No predictions for this date.</p>${source()}`);
    }

    const top = Math.ceil(Math.max(1, ...tides.series.map(([, l]) => l)));
    const y = (level) => 100 - (Math.max(0, level) / top) * 92;
    const points = tides.series
      .filter(([t]) => {
        const ms = Date.parse(t);
        return ms >= this.start && ms <= this.end;
      })
      .map(([t, l]) => `${(this.pct(t) * 10).toFixed(2)},${y(l).toFixed(2)}`)
      .join(" ");
    const curve = `<svg viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true">
      <polygon class="water" points="0,100 ${points} 1000,100"/>
      <polyline class="surface" points="${points}"/>
    </svg>`;

    // Low waters are labelled in the air above the trough, high waters in
    // white inside the water under the crest.
    const labels = tides.extremes
      .map((e) => {
        const pct = this.pct(e.time);
        const align = pct < 6 ? " start" : pct > 94 ? " end" : "";
        return `<span class="mark ${e.kind}${align}" style="left:${pct}%">${this.hm(e.time)} · ${e.level.toFixed(2)} m</span>`;
      })
      .join("");

    const hourly = tides.hourly
      .map((l, h) => `<div><span>${pad(h)}</span>${l == null ? "–" : l.toFixed(1)}</div>`)
      .join("");

    return section(
      "Tides",
      this.timeline("sea", curve + labels, { axis: false }) +
        `<div class="hourly">${hourly}</div>` +
        source(` · scale 0–${top} m`),
    );
  }
}

main();
