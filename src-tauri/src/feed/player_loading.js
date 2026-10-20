// A spinner over the play page in the player window (D-067) until the
// embedded player's frame has loaded: until then WebView2 shows the frame
// black. It is the launcher's, so the play page needs no script of its own.
// It runs only in the play page itself, never in the players' frames.
(function () {
  'use strict';
  if (window.top !== window || location.host !== 'feed.lapi.sh') return;
  document.addEventListener('DOMContentLoaded', function () {
    var frame = document.querySelector('iframe');
    if (!frame) return;
    var wait = document.createElement('div');
    wait.setAttribute('aria-hidden', 'true');
    wait.style.cssText =
      'position:fixed;top:50%;left:50%;width:44px;height:44px;margin:-22px 0 0 -22px;' +
      'box-sizing:border-box;border:3px solid #2a2a2a;border-top-color:#9cf;border-radius:50%;' +
      'z-index:2147483647;pointer-events:none;transition:opacity .3s';
    var reduced =
      window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    wait.animate([{ transform: 'rotate(0turn)' }, { transform: 'rotate(1turn)' }], {
      duration: reduced ? 3000 : 900,
      iterations: Infinity,
    });
    document.body.appendChild(wait);
    frame.addEventListener('load', function () {
      wait.style.opacity = '0';
      setTimeout(function () {
        wait.remove();
      }, 400);
    });
  });
})();
