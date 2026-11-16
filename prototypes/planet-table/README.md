# A planet, without the wait

A 2 minute 44 second silent, captioned HTML/SVG + GSAP explainer. This is an
educational prototype, not a browser client for Night Sky. No game code changes.

Open `index.html` in a browser. GSAP 3.13.0 loads from jsDelivr and optional fonts
load from Google Fonts, so the first load needs an internet connection. There is
no build step. Play/pause, chapters, speed, scrubbing and fullscreen are available.
Space toggles playback when focus is not on a control; arrows seek five seconds.
The page starts paused. It does not generate an MP4 or include recorded narration.

## Sources and scope

Checked against the latest Night Sky-related Claude CLI session
(`3bd02941-4c1d-4b25-912f-13849a6c751d`) and the working tree on 16 November 2026.
Session content is context, not the implementation authority. Canonical guide:
[structure table](../../docs/simulation/planet-composition.md#structure-table).

- The condensed table has axes mass, metal/refractory share, water/total share,
  and water temperature. Each lookup blends up to 16 knots, storing ln(radius)
  and inertia factor. Log mass coordinates determine mass weights.
- The real example embeds 16 rounded entries extracted from the existing
  `NSSTR001` resource. Its coordinates are mass knots 30/31, metal knots 2/3,
  water knots 7/8, temperature knots 12/13. Radius is shown in km, so taking
  ln(km) and exponentiating gives the same blend as ln(m), up to rounding.
  This is a standalone condensed-radius calculation, not a full generated
  planet's atmosphere/retention calculation. The example does not run Rust.
- The first diagrams are illustrative. A coloured tile represents a stored
  knot, not a whole constant-value cell. The four-dimensional picture is two
  3D slices; 4D does not need to be projected as a hypercube.
- Current uncommitted work is widening the core grid with a pressure-load axis
  and adding envelope outputs. The film deliberately avoids presenting the
  stale resource size, old core-grid dimensions or a new build's accuracy as
  current facts. The loaded-core diagram is conceptual, not a measured claim.
- Current generation already gets the reference radius from tables, but
  retention, atmosphere decisions and other direct calculations remain.
  Background refinement and adoption still exist. Removing the swap or the
  runtime solver is a goal, not a completed change.
- Filled cells and out-of-domain lookups are approximate. A table interpolates
  the physical model; it is not an exact solution or a guarantee of real-world
  accuracy. No performance measurements are invented for the film.

## Deterministic frames

In the browser console, `planetLesson.seek(115)` pauses at an exact second.
`planetLesson.timeline` exposes the GSAP timeline. All animation is on that
timeline, with no random state or independent timers. `query`, `bounds`,
`weights`, and `result` expose the worked example for inspection.
