# Web and Docs Search

Enable Cascade to search the web and read documentation pages in real-time using @web and @docs mentions for up-to-date context.

Cascade can now intuitively parse through and chunk up web pages and documentation, providing realtime context to the models. The key way to understand this feature is that Cascade will browse the Internet as a human would.

Our web tools are designed in such a way that gets only the information that is necessary in order to efficiently use your credits.

## Overview

To help you better understand how Web Search works, we've recorded a short video covering the key concepts and best practices.

<iframe title="YouTube video player" />

### Quick Start

The fastest way to get started is to activate web search in your Windsurf Settings in the bottom right corner of the editor. You can activate it a couple of different ways:

1. Ask a question that probably needs the Internet (ie. "What's new in the latest version of React?").
2. Use `@web` to force a docs search.
3. Use `@docs` to query over a list of docs that we are confident we can read with high quality.
4. Paste a URL into your message.

## Search the web

Cascade can deduce that certain prompts from the user may require a real-time web search to provide the optimal response. In these cases, Cascade will perform a web search and provide the results to the user. This can happen automatically or manually using the `@web` mention.

<Note>
  The **Enable Web Search** admin setting controls whether Cascade can perform web searches on the open Internet. It does not affect Cascade's ability to read specific URLs (see [Reading Pages](#reading-pages) below), which is performed locally on the user's machine.
</Note>

<Frame>
  <img />
</Frame>

## Reading Pages

Cascade can read individual pages for things like documentation, blog posts, and GitHub files. The page reads happen entirely on your device within your network so if you're using a VPN you shouldn't have any problems.

Pages are picked up either from web search results, inferred based on the conversation, or from URLs pasted directly into your message.

We break pages up into multiple chunks, very similar to how a human would read a page: for a long page we skim to the section we want then read the text that's relevant. This is how Cascade operates as well.

<Frame>
  <img />
</Frame>

It's worth noting that not all pages can be parsed. We are actively working on improving the quality of our website reading. If you have specific sites you'd like us to handle better, feel free to file a feature request!
