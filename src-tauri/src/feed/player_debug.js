// What the player window's frames report to the launcher's console (D-067).
// It runs in every frame of the player window: the play page and the embedded
// players. Frames send their lines to the play page, which shows each one
// briefly as its document title behind a marker; the launcher logs those
// titles and nothing else. It changes nothing in the pages it watches but
// console.error, which it still calls.
(function () {
  'use strict';
  var KEY = 'launcherPlayerDebug';
  var MARK = '\u2063player\u2063';
  var here = location.host || location.href.slice(0, 40);

  function text(value, max) {
    try {
      return String(value).replace(/[\u0000-\u001f\u007f]+/g, ' ').slice(0, max || 200);
    } catch (e) {
      return '?';
    }
  }

  var send;
  if (window.top !== window) {
    send = function (line) {
      try {
        var message = {};
        message[KEY] = here + ': ' + line;
        window.top.postMessage(message, '*');
      } catch (e) {}
    };
  } else {
    var queue = [];
    var sent = 0;
    var timer = 0;
    var saved = null;
    var pump = function () {
      timer = 0;
      if (document.readyState === 'loading') {
        timer = setTimeout(pump, 100);
        return;
      }
      if (saved === null) saved = document.title;
      if (queue.length) {
        document.title = MARK + queue.shift();
        timer = setTimeout(pump, 60);
      } else {
        document.title = saved;
        saved = null;
      }
    };
    var post = function (line) {
      if (sent >= 80) return;
      sent += 1;
      queue.push(text(line, 300));
      if (!timer) timer = setTimeout(pump, 0);
    };
    send = function (line) {
      post(here + ': ' + line);
    };
    window.addEventListener('message', function (event) {
      var data = event.data;
      if (data && typeof data === 'object' && typeof data[KEY] === 'string') post(data[KEY]);
    });
    window.addEventListener('load', function () {
      var frames = document.getElementsByTagName('iframe');
      for (var i = 0; i < frames.length; i += 1) {
        var box = frames[i].getBoundingClientRect();
        send('frame ' + text(frames[i].src, 120) + ' is ' + Math.round(box.width) + 'x' + Math.round(box.height));
      }
    });
  }

  window.addEventListener('error', function (event) {
    var target = event.target;
    if (target && target !== window && (target.src || target.href)) {
      send('failed to load ' + text(target.tagName) + ' ' + text(target.src || target.href, 150));
    } else {
      send('script error ' + text(event.message) + ' at ' + text(event.filename, 120) + ':' + event.lineno);
    }
  }, true);
  window.addEventListener('unhandledrejection', function (event) {
    var reason = event.reason;
    send('unhandled rejection ' + text(reason && reason.message ? reason.message : reason));
  });
  document.addEventListener('securitypolicyviolation', function (event) {
    send('CSP blocked ' + text(event.blockedURI, 150) + ' (' + text(event.violatedDirective) + ')');
  });
  var consoleError = console.error;
  console.error = function () {
    try {
      send('console.error ' + text(Array.prototype.join.call(arguments, ' ')));
    } catch (e) {}
    return consoleError.apply(this, arguments);
  };
  document.addEventListener('DOMContentLoaded', function () {
    send('document ready');
  });
  window.addEventListener('load', function () {
    send('loaded');
  });
})();
