# devpage authenticates no one

<Callout kind="tip" title="A review rule, written for the AI reviewer">

This file is a constraint for automated AI code review. On every change to this box or to anything inside it, the
reviewer reads it and checks the diff against the rule below; a change that
breaks it is a finding. People can read it too, but it is written to be
checked: the rule, why it holds, and how a breach shows.

</Callout>

<Callout kind="note" title="The rule">

devpage has no login, no sessions and no user list. Running locally — on a
loopback address — the author of a comment is `git config user.name`
as git sees it in the served folder, or the system's user name when git has none. Deployed — on any other address — the author is the
value of one request header, `X-Forwarded-Email` unless `--user-header` names
another, set by the auth proxy in front of it, trimmed; a request that writes a
comment without that header, or with it blank, is refused with `401`. Reading needs no header: whoever the
proxy lets through may read.

In [server mode](../server/README.md) the header is optional: with
`DEVPAGE_USER_HEADER` set it names the author of every publish and comment and
a write without it, or with it blank, gets `401`; without it, writes are `anonymous`. The CLI may
send a bearer token from its context's token command; devpage never reads it —
it is for the proxy.

</Callout>

## Why

A team that deploys docs already has an identity provider and a proxy
in front of its services. Every login devpage built would be one more to
secure and maintain, and still worse than the one they have. Trusting the
proxy's header keeps devpage free of credentials — at the price that the header
means nothing without the proxy: anyone who can reach devpage directly can set
it.

## Check

No code stores or checks a password, token or session. A deployed
devpage refuses a comment write that lacks the header or leaves it blank, and
stamps the header's trimmed value as the author when it is present — one rule,
`comments::Identity`, for a local page's writes and every server-mode write. The deploying page says devpage must
only be reachable through the proxy.
