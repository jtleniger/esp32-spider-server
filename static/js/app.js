// Renders the activation histogram for the day baked into the canvas data attributes.
// Everything is local: the JSON comes from /logs/data on this server, Chart.js from
// the embedded vendor bundle.
(function () {
  var canvas = document.getElementById('activation-chart');
  if (!canvas) return;

  var url = canvas.dataset.url + '?day=' + encodeURIComponent(canvas.dataset.day);

  // The server buckets each UTC five-minute slot; draw the axis in the viewer's
  // timezone so a local evening activation is not plotted as a small-hours one.
  var localTime = new Intl.DateTimeFormat([], { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
  function localLabel(day, hhmm) {
    return localTime.format(new Date(day + 'T' + hhmm + ':00Z'));
  }

  fetch(url)
    .then(function (r) {
      if (!r.ok) throw new Error('HTTP ' + r.status);
      return r.json();
    })
    .then(function (d) {
      new Chart(canvas, {
        type: 'bar',
        data: {
          labels: d.labels.map(function (t) { return localLabel(d.day, t); }),
          datasets: [{
            label: 'activations',
            data: d.counts,
            backgroundColor: '#ff7a18',
            borderColor: '#ff7a18',
            borderWidth: 1
          }]
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          animation: false,
          barPercentage: 1.0,
          categoryPercentage: 0.98,
          plugins: { legend: { display: false }, tooltip: { displayColors: false } },
          scales: {
            x: {
              ticks: { color: '#9a92a6', maxTicksLimit: 12, autoSkip: true },
              grid: { color: '#2c2836' }
            },
            y: {
              beginAtZero: true,
              ticks: { color: '#9a92a6', precision: 0 },
              grid: { color: '#2c2836' }
            }
          }
        }
      });
    })
    .catch(function () {
      canvas.insertAdjacentHTML('afterend', '<p class="banner error">could not load log data</p>');
    });
})();
