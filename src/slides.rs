const SLIDES_SCRIPT: &str = r##"<script>
(function () {
  if (window.__rendarSlides) {
    return;
  }
  window.__rendarSlides = true;

  var slides = Array.prototype.slice.call(document.querySelectorAll(".slide"));
  if (!slides.length) {
    return;
  }
  var progress = document.querySelector(".slides-progress");
  var current = 0;

  function clamp(index) {
    if (index < 0) {
      return 0;
    }
    if (index >= slides.length) {
      return slides.length - 1;
    }
    return index;
  }

  function parseHash() {
    var match = window.location.hash.match(/slide-(\d+)/);
    if (!match) {
      return 0;
    }
    var value = parseInt(match[1], 10);
    if (Number.isNaN(value)) {
      return 0;
    }
    return value - 1;
  }

  function updateProgress(index) {
    if (!progress) {
      return;
    }
    progress.textContent = (index + 1) + " / " + slides.length;
  }

  function show(index, updateHash) {
    var next = clamp(index);
    slides[current].classList.remove("is-active");
    slides[current].setAttribute("aria-hidden", "true");
    slides[next].classList.add("is-active");
    slides[next].removeAttribute("aria-hidden");
    current = next;
    updateProgress(current);
    if (updateHash) {
      var hash = "#slide-" + (current + 1);
      if (window.location.hash !== hash) {
        window.location.hash = hash;
      }
    }
  }

  function nextSlide() {
    show(current + 1, true);
  }

  function previousSlide() {
    show(current - 1, true);
  }

  function shouldIgnoreEvent(event) {
    var target = event.target;
    if (!target) {
      return false;
    }
    var tag = target.tagName ? target.tagName.toLowerCase() : "";
    return tag === "input" || tag === "textarea" || target.isContentEditable;
  }

  document.addEventListener("keydown", function (event) {
    if (event.defaultPrevented || shouldIgnoreEvent(event)) {
      return;
    }
    if (event.key === "ArrowRight" || event.key === " " || event.key === "Spacebar") {
      event.preventDefault();
      nextSlide();
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      previousSlide();
    }
  });

  window.addEventListener("hashchange", function () {
    show(parseHash(), false);
  });

  show(parseHash(), false);
})();
</script>
"##;

const SLIDES_MODE_SCRIPT: &str = r#"<script>
document.documentElement.classList.add("slides-mode");
</script>
"#;

const SLIDES_STYLE: &str = include_str!("../assets/theme/slides.css");

pub fn slides_extra_head() -> String {
    format!(
        "{}<style>\n{}</style>\n",
        SLIDES_MODE_SCRIPT, SLIDES_STYLE
    )
}

pub fn slides_extra_body() -> &'static str {
    SLIDES_SCRIPT
}
