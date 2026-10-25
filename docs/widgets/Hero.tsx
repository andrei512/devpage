// A title slide: the logo, a huge title, a line under it, and a kicker above.
export const css = `
  .hero {
    position: absolute; inset: 0;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 36px;
    text-align: center;
    background:
      radial-gradient(1200px 600px at 50% 110%, color-mix(in srgb, var(--accent) 18%, transparent), transparent 70%),
      var(--background);
  }
  .hero img { width: 200px; height: 200px; filter: drop-shadow(0 24px 48px color-mix(in srgb, var(--accent) 35%, transparent)); }
  .hero-kicker { font-size: 34px; font-weight: 600; letter-spacing: 0.14em; text-transform: uppercase; color: var(--accent); }
  .hero-title { font-size: 196px; font-weight: 750; letter-spacing: -0.045em; line-height: 1; margin: 0; }
  .hero-subtitle { font-size: 56px; color: var(--muted); max-width: 1400px; line-height: 1.25; }
`;

export default function Hero({ title, subtitle, kicker, logo }: { title: string; subtitle?: string; kicker?: string; logo?: string }) {
  return (
    <div className="hero">
      {logo ? <img src={logo} alt="" /> : null}
      {kicker ? <div className="hero-kicker">{kicker}</div> : null}
      <h1 className="hero-title">{title}</h1>
      {subtitle ? <div className="hero-subtitle">{subtitle}</div> : null}
    </div>
  );
}
