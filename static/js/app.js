// Renders the activation histogram for the day baked into the canvas data attributes.
// Everything is local: the JSON comes from /logs/data on this server, Chart.js from
// the embedded vendor bundle.
(function () {
  var canvas = document.getElementById('activation-chart');
  if (!canvas) return;

  var url = canvas.dataset.url + '?day=' + encodeURIComponent(canvas.dataset.day);

  fetch(url)
    .then(function (r) {
      if (!r.ok) throw new Error('HTTP ' + r.status);
      return r.json();
    })
    .then(function (d) {
      new Chart(canvas, {
        type: 'bar',
        data: {
          labels: d.labels,
          datasets: [{
            label: 'activations',
            data: d.counts,
            backgroundColor: 'rgba(255,117,24,.75)',
            borderColor: '#ff7518',
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
              ticks: { color: '#8a8377', maxTicksLimit: 12, autoSkip: true },
              grid: { color: '#2a2620' }
            },
            y: {
              beginAtZero: true,
              ticks: { color: '#8a8377', precision: 0 },
              grid: { color: '#2a2620' }
            }
          }
        }
      });
    })
    .catch(function () {
      canvas.insertAdjacentHTML('afterend', '<p class="banner error">could not load log data</p>');
    });
})();
