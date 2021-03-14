"use strict";
// eslint-disable-next-line @typescript-eslint/no-unused-vars
function registerModification(manifestDirBlobURL, license, cargoAddCommand, dependencyUL, codeSizeUnmodified, verifiedWith) {
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
        docblock.prepend(createDependenciesSection(dependencyUL));
        docblock.prepend(createHeader("Dependencies", "dependencies"));
        docblock.prepend(createCargoAddCommandSection(cargoAddCommand));
        docblock.prepend(createFirstSection(manifestDirBlobURL, license));
    });
}
function findOrCreateDocblock() {
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
function createToggleWrapper() {
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
function downgradeSectionHeaders(docblock) {
    docblock.querySelectorAll(".section-header").forEach((sectionHeader) => {
        const replacement = document.createElement((() => {
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
        })());
        for (const { name, value } of sectionHeader.attributes) {
            replacement.setAttribute(name, value);
        }
        replacement.append(...sectionHeader.childNodes);
        sectionHeader.replaceWith(replacement);
    });
}
function createVerifiedWithSection(verifiedWith) {
    const div = document.createElement("div");
    switch (verifiedWith.length) {
        case 0: {
            const strong = document.createElement("strong");
            strong.append(createWarningMark(), " This library is not verified.");
            div.append(strong);
            break;
        }
        case 1:
            div.append(createHeavyCheckMark(), " This library verified with 1 solution.");
            break;
        default:
            div.append(createHeavyCheckMark(), " This library verified with " + verifiedWith.length + " solutions.");
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
function createHeavyCheckMark() {
    return createMark("https://github.githubassets.com/images/icons/emoji/unicode/2714.png", "✔");
}
function createWarningMark() {
    return createMark("https://github.githubassets.com/images/icons/emoji/unicode/26a0.png", "⚠");
}
function createMark(src, char) {
    const mark = document.createElement("img");
    mark.setAttribute("src", src);
    mark.setAttribute("alt", char);
    mark.setAttribute("title", char);
    mark.setAttribute("width", "20");
    mark.setAttribute("height", "20");
    return mark;
}
function createCodeSizeSection(codeSizeUnmodified) {
    const ul = document.createElement("ul");
    const li1 = document.createElement("li");
    li1.append("unmodified: ");
    if (typeof codeSizeUnmodified === "number") {
        const div = Math.floor(codeSizeUnmodified / 1024);
        const rem = codeSizeUnmodified % 1024;
        li1.append("" + div + "." + Math.floor(10 * rem / 1024) + " KiB");
    }
    else {
        const code = document.createElement("code");
        code.append(codeSizeUnmodified);
        li1.append(code);
    }
    li1.append(" + (not yet implemented) KiB");
    const li2 = document.createElement("li");
    const li3 = document.createElement("li");
    const code1 = document.createElement("code");
    const code2 = document.createElement("code");
    code1.append("#[cfg]");
    code2.append("#[cfg]");
    li2.append(code1, " resolved + (doc-)comment removed + Rustfmt: (not yet implemented)");
    li3.append(code2, " resolved + doc-comment removed + minified: (not yet implemented)");
    ul.append(li1, li2, li3);
    return ul;
}
function createDependenciesSection(items) {
    if (items.length === 0) {
        return "No dependencies.";
    }
    const ul = document.createElement("ul");
    for (const [text, href] of items) {
        const li = document.createElement("li");
        const a = document.createElement("a");
        a.setAttribute("href", href);
        a.append(text);
        li.append(a);
        ul.append(li);
    }
    return ul;
}
function createCargoAddCommandSection(cargoAddCommand) {
    const pre = document.createElement("pre");
    const code = document.createElement("code");
    code.setAttribute("class", "language-console");
    code.append("$ " + cargoAddCommand + "\n");
    pre.append(code);
    return pre;
}
function createFirstSection(manifestDirBlobURL, license) {
    const ul = document.createElement("ul");
    const li1 = document.createElement("li");
    const a = document.createElement("a");
    const li2 = document.createElement("li");
    const code = document.createElement("code");
    a.setAttribute("href", manifestDirBlobURL);
    a.append("View on GitHub");
    li1.append(a);
    code.append(license);
    li2.append("License: ", code);
    ul.append(li1, li2);
    return ul;
}
function createHeader(name, id) {
    const header = document.createElement("h1");
    header.setAttribute("id", id);
    header.setAttribute("class", "section-header");
    const a = document.createElement("a");
    a.setAttribute("href", "#" + id);
    a.append(document.createTextNode(name));
    header.append(a);
    return header;
}
