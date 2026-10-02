// Helpers shared by the ShoreCast pages.

export const pad = (n) => String(n).padStart(2, "0");

export function esc(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** "YYYY-MM-DD" → [year, month (1-12), day], or null. */
export function parseIsoDate(s) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

/** Calendar date as "YYYY-MM-DD" (month 1-12; day may overflow). */
export function isoDate(year, month, day) {
  const d = new Date(Date.UTC(year, month - 1, day));
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

export function addDays(iso, n) {
  const [y, m, d] = parseIsoDate(iso);
  return isoDate(y, m, d + n);
}

/** Today's date at the camera point, as "YYYY-MM-DD". */
export function todayIn(tz, now = new Date()) {
  // en-CA formats dates as YYYY-MM-DD.
  return new Intl.DateTimeFormat("en-CA", { timeZone: tz }).format(now);
}

export function monthTitle(year, month) {
  return new Date(Date.UTC(year, month - 1, 1)).toLocaleDateString("en-GB", {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
}

export async function getJson(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r.json();
}

/** Icon, name and strip colour (dark theme) for a WMO weather code. */
export function weatherInfo(code, isDay = true) {
  if (code === 0) return isDay ? ["☀️", "Clear", "#5a4712"] : ["🌙", "Clear", "#1a2233"];
  if (code === 1) return isDay ? ["🌤️", "Mainly clear", "#4b4120"] : ["🌙", "Mainly clear", "#1d2433"];
  if (code === 2) return isDay ? ["⛅", "Partly cloudy", "#353a43"] : ["☁️", "Partly cloudy", "#262b34"];
  if (code === 3) return ["☁️", "Overcast", "#2c3038"];
  if (code === 45 || code === 48) return ["🌫️", "Fog", "#34383e"];
  if (code >= 51 && code <= 57) return ["🌦️", "Drizzle", "#24384c"];
  if (code >= 61 && code <= 64) return ["🌧️", "Rain", "#1f3d5c"];
  if (code >= 65 && code <= 67) return ["🌧️", "Heavy rain", "#173357"];
  if (code >= 71 && code <= 77) return ["🌨️", "Snow", "#45505e"];
  if (code >= 80 && code <= 82) return ["🌦️", "Showers", "#22456a"];
  if (code >= 85 && code <= 86) return ["🌨️", "Snow showers", "#3e4a59"];
  if (code >= 95) return ["⛈️", "Thunderstorm", "#3b3263"];
  return ["❔", "Unknown", "#2a2a2a"];
}

/** Local "HH:MM" formatter for a time zone. */
export function clockIn(tz) {
  const f = new Intl.DateTimeFormat("en-GB", {
    timeZone: tz,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });
  return (iso) => (iso ? f.format(new Date(iso)) : "–");
}
