function modifyDocblock(
  manifestRelPath: string,
  manifestHref: string,
  license: string,
  cargoAddCommand: string,
  codeSizeUnmodified: string | null,
  verifiedWith: [string, string][],
) {
  const docblock = document.querySelector(".docblock");
  if (docblock) {
    downgradeSectionHeaders(docblock);
    docblock.prepend(createHeader("Description", "description"));
    docblock.prepend(createVerifiedWithSection(verifiedWith));
    docblock.prepend(createHeader("Verified with", "verified-with"));
    if (codeSizeUnmodified !== null) {
      docblock.prepend(createCodeSizeSection(codeSizeUnmodified));
      docblock.prepend(createHeader("Code size", "code-size"));
    }
    docblock.prepend(createDependencySection(cargoAddCommand));
    docblock.prepend(createFirstSection(manifestRelPath, manifestHref, license));
  }
}

function downgradeSectionHeaders(docblock: Element) {
  docblock.querySelectorAll(".section-header").forEach((sectionHeader) => {
    const replacement = document.createElement((() => {
      switch (sectionHeader.tagName) {
        case 'H1': return 'H2';
        case 'H2': return 'H3';
        case 'H3': return 'H4';
        case 'H4': return 'H5';
        case 'H5': return 'H6';
        default: return sectionHeader.tagName;
      }
    })());
    for (const { name, value } of sectionHeader.attributes) {
      replacement.setAttribute(name, value);
    }
    replacement.append(...sectionHeader.childNodes);
    sectionHeader.replaceWith(replacement);
  });
}

function createVerifiedWithSection(verifiedWith: [string, string][]): HTMLElement {
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
  return ul;
}

function createCodeSizeSection(codeSizeUnmodified: string): HTMLElement {
  const ul = document.createElement("ul");
  const li1 = document.createElement("li");
  li1.append("unmodified: " + codeSizeUnmodified + " KiB + (not yet implemented) KiB");
  const li2 = document.createElement("li");
  li2.append("")
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

function createFirstSection(manifestRelPath: string, manifestHref: string, license: string): HTMLElement {
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
