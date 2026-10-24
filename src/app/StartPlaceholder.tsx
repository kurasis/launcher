/**
 * The start placeholder's screen and launch bar, as index.html draws them
 * before any script runs (src/styles/global.css). The shell shows them while
 * the core's state loads, so the shape on screen stays the same from the
 * window's first frame until the feed draws. Keep the markup as in index.html;
 * a test compares the two.
 */
export function StartScreen() {
  return (
    <div className="boot-screen" aria-hidden="true">
      <div className="boot-head">
        <span />
        <div>
          <span />
          <span />
        </div>
      </div>
      <div className="boot-panels">
        <span />
        <span />
      </div>
      <div className="boot-chips">
        <span />
        <span />
        <span />
        <span />
        <span />
      </div>
      <div className="boot-mosaic">
        <div>
          <span />
          <span />
        </div>
        <div>
          <span />
          <span />
        </div>
        <div>
          <span />
          <span />
        </div>
      </div>
    </div>
  );
}

export function StartBar() {
  return (
    <div className="boot-bar" aria-hidden="true">
      <span />
    </div>
  );
}
