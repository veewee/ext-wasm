# Link previews and reader mode

This example is a Rust component that fetches web pages itself. Given a URL, it returns what a chat app shows when someone pastes a link (title, description, image), or the main content of the page as Markdown, for reading or for handing to an LLM. It shows both sides of HTTP in a component:

- The component makes its own requests through `wasi:http`, and PHP decides which hosts it may reach. Any other host fails inside the component before a request goes out, a redirect to one included. Hosts are matched by name, not by the address they resolve to, and a rule without a port allows every port of that host, so `host:port` is the tight form. `preview.php` allows only the host and port of the URL it is given.
- The component also answers HTTP requests as a `wasi:http/incoming-handler`. PHP receives the request, here from `php -S`, and hands it over, so routing, authentication and caching stay in PHP.

`src/lib.rs` uses [scraper](https://github.com/rust-scraper/scraper) to read the page and [htmd](https://github.com/letmutex/htmd) to turn HTML into Markdown. The reader mode takes the page's `article` or `main` element, or its body, and leaves out navigation, headers, footers, scripts and forms. That is a simple heuristic, not a full readability algorithm, so pages with a lot of layout around the text come out noisier.

The component guards itself against hostile pages: it reads at most 5 MiB, gives the whole fetch, redirects included, 20 seconds, and refuses a page whose elements nest more than 256 levels deep before parsing it, because parsing and converting recurse over the tree and a much deeper page would exhaust the stack and trap the instance. Its output is still content from someone else's page: the image is always an http or https URL, but links in the Markdown keep whatever the page used, so sanitise the Markdown before rendering it as HTML.

## Running it

Install the extension as described in the [main README](../../README.md). The compiled `link-preview.wasm` is checked in, so running it needs no Rust.

From the command line, the component may only reach the host of the URL you give:

```sh
php examples/link-preview/preview.php https://www.php.net/
php examples/link-preview/preview.php https://en.wikipedia.org/wiki/WebAssembly --read
```

As a service behind PHP's built-in server, allowing the hosts in `LINK_PREVIEW_HOSTS`:

```sh
LINK_PREVIEW_HOSTS='www.php.net,*.wikipedia.org' php -S localhost:8000 examples/link-preview/server.php
curl 'localhost:8000/preview?url=https://www.php.net/'
curl 'localhost:8000/read?url=https://en.wikipedia.org/wiki/WebAssembly'
```

`/preview` answers JSON and `/read` Markdown, for GET only. A page on a host that is not allowed, or one the component refuses, answers 502 with the reason. `server.php` creates an instance per request, so a trap only fails that request, with a 500.

## From PHP

`LinkPreview.php` wraps the component:

```php
$pages = new LinkPreview(['www.php.net', '*.wikipedia.org']);
$pages->preview('https://www.php.net/');   // ['url' => ..., 'title' => 'PHP', 'description' => ..., 'image' => ..., 'siteName' => null]
$pages->read('https://en.wikipedia.org/wiki/WebAssembly');   // Markdown
$pages->handle($request);                  // a Wasm\Component\Http\Request, answered with a Response
```

`preview()` returns the WIT record `page-preview` as an array with camelCase keys, and a failed fetch throws a `Wasm\Exception\ComponentError` whose `payload` is the reason.

## Changing the Rust code

Install Rust from [rustup.rs](https://rustup.rs), edit `src/lib.rs` and rebuild:

```sh
examples/link-preview/build.sh
```

`wit/pages.wit` declares the typed functions, which wit-bindgen generates the Rust side for. The HTTP handler and the outgoing requests come from the `wasi` crate, which declares those interfaces itself.
