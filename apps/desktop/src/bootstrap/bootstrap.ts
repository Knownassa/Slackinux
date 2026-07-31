const errorState = document.getElementById("error-state");
const errorMessage = document.getElementById("error-message");
const loadingState = document.getElementById("loading-state");

function showError(msg: string) {
  if (loadingState) loadingState.style.display = "none";
  if (errorState) errorState.style.display = "block";
  if (errorMessage) errorMessage.textContent = msg;
}

// The Rust backend navigates this window to Slack on successful startup.
// If we detect a navigation failure or timeout, show an error.
let navTimeout = window.setTimeout(() => {
  showError(
    "Slack did not load within the expected time. Check your network connection and try restarting the application.",
  );
}, 30000);

// Cancel the timeout when the page is hidden (navigated away from),
// which happens when Rust navigates to Slack.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    window.clearTimeout(navTimeout);
  }
});

// Expose showError globally so Rust can call it via eval() on failure.
(window as unknown as Record<string, unknown>).showSlackinuxError = showError;
