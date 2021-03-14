// eslint-disable-next-line @typescript-eslint/no-unused-vars
function registerModification(
  manifestRelPath: string,
  manifestHref: string,
  license: string,
  cargoAddCommand: string,
  codeSizeUnmodified: string | null,
  verifiedWith: [string, string][]
): void {
  if (!window.location.pathname.endsWith("/index.html")) {
    return;
  }
  document.addEventListener("DOMContentLoaded", () => {
    const docblock = findOrCreateDocblock();
    if (docblock === null) {
      return;
    }
    downgradeSectionHeaders(docblock);
    docblock.prepend(createHeader("Description", "description"));
    docblock.prepend(createVerifiedWithSection(verifiedWith));
    docblock.prepend(createHeader("Verified with", "verified-with"));
    if (codeSizeUnmodified !== null) {
      docblock.prepend(createCodeSizeSection(codeSizeUnmodified));
      docblock.prepend(createHeader("Code size", "code-size"));
    }
    docblock.prepend(createDependencySection(cargoAddCommand));
    docblock.prepend(
      createFirstSection(manifestRelPath, manifestHref, license)
    );
  });
}

function findOrCreateDocblock(): Element | null {
  let docblock = document.querySelector(".docblock");
  if (docblock !== null) {
    return docblock;
  }
  const fqn = document.querySelector(".fqn");
  if (fqn === null) {
    return null;
  }
  docblock = document.createElement("div");
  docblock.setAttribute("class", "docblock");
  fqn.insertAdjacentElement("afterend", docblock);
  fqn.insertAdjacentElement("afterend", createToggleWrapper());
  return docblock;
}

function createToggleWrapper(): Element {
  const toggleWrapper = document.createElement("div");
  const a = document.createElement("a");
  const span1 = document.createElement("span");
  const span2 = document.createElement("span");
  toggleWrapper.setAttribute("class", "toggle-wrapper");
  a.setAttribute("class", "collapse-toggle");
  a.setAttribute("href", "javascript:void(0)");
  span1.setAttribute("class", "inner");
  span2.setAttribute("class", "toggle-label");
  span2.setAttribute("style", "display: none;");
  span1.append("-");
  span2.append(" Expand description");
  a.append("[", span1, "] ", span2);
  toggleWrapper.append(a);
  return toggleWrapper;
}

function downgradeSectionHeaders(docblock: Element): void {
  docblock.querySelectorAll(".section-header").forEach((sectionHeader) => {
    const replacement = document.createElement(
      (() => {
        switch (sectionHeader.tagName) {
          case "H1":
            return "H2";
          case "H2":
            return "H3";
          case "H3":
            return "H4";
          case "H4":
            return "H5";
          case "H5":
            return "H6";
          default:
            return sectionHeader.tagName;
        }
      })()
    );
    for (const { name, value } of sectionHeader.attributes) {
      replacement.setAttribute(name, value);
    }
    replacement.append(...sectionHeader.childNodes);
    sectionHeader.replaceWith(replacement);
  });
}

function createVerifiedWithSection(
  verifiedWith: [string, string][]
): HTMLDivElement {
  const div = document.createElement("div");
  switch (verifiedWith.length) {
    case 0: {
      const strong = document.createElement("strong");
      strong.append(createWarningMark(), " This library is not verified.");
      div.append(strong);
      break;
    }
    case 1:
      div.append(
        createHeavyCheckMark(),
        " This library verified with 1 solution."
      );
      break;
    default:
      div.append(
        createHeavyCheckMark(),
        " This library verified with " + verifiedWith.length + " solutions."
      );
  }
  const ul = document.createElement("ul");
  for (const [problemURL, blobURL] of verifiedWith) {
    const li = document.createElement("li");
    const a1 = document.createElement("a");
    a1.setAttribute("href", problemURL);
    a1.append(problemURL);
    const a2 = document.createElement("a");
    a2.setAttribute("href", blobURL);
    a2.append("code");
    li.append(a1, " (", a2, ")");
    ul.append(li);
  }
  div.append(ul);
  return div;
}

function createHeavyCheckMark(): HTMLImageElement {
  return createMark(
    "https://github.githubassets.com/images/icons/emoji/unicode/2714.png",
    "✔"
  );
}

function createWarningMark(): HTMLImageElement {
  return createMark(
    "https://github.githubassets.com/images/icons/emoji/unicode/26a0.png",
    "⚠"
  );
}

function createMark(src: string, char: string): HTMLImageElement {
  const mark = document.createElement("img");
  mark.setAttribute("src", src);
  mark.setAttribute("alt", char);
  mark.setAttribute("title", char);
  mark.setAttribute("width", "20");
  mark.setAttribute("height", "20");
  return mark;
}

function createCodeSizeSection(codeSizeUnmodified: string): HTMLElement {
  const ul = document.createElement("ul");
  const li1 = document.createElement("li");
  li1.append(
    "unmodified: " + codeSizeUnmodified + " KiB + (not yet implemented) KiB"
  );
  const li2 = document.createElement("li");
  li2.append("");
  ul.append(li1);
  return ul;
}

function createDependencySection(cargoAddCommand: string): HTMLElement {
  const pre = document.createElement("pre");
  const code = document.createElement("code");
  code.setAttribute("class", "language-console");
  code.append("$ " + cargoAddCommand + "\n");
  pre.append(code);
  return pre;
}

function createFirstSection(
  manifestRelPath: string,
  manifestHref: string,
  license: string
): HTMLElement {
  const ul = document.createElement("ul");
  const li1 = document.createElement("li");
  const a = document.createElement("a");
  a.setAttribute("href", manifestHref);
  const code1 = document.createElement("code");
  code1.append(manifestRelPath);
  a.append(code1);
  li1.append("Manifest: ", a);
  const li2 = document.createElement("li");
  const code2 = document.createElement("code");
  code2.append(license);
  li2.append("License: ", code2);
  ul.append(li1, li2);
  return ul;
}

function createHeader(name: string, id: string): HTMLElement {
  const header = document.createElement("h1");
  header.setAttribute("id", id);
  header.setAttribute("class", "section-header");
  const a = document.createElement("a");
  a.setAttribute("href", "#" + id);
  a.append(document.createTextNode(name));
  header.append(a);
  return header;
}
