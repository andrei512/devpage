# devpage authenticates no one

devpage has no login, no sessions and no user list. Running locally — on a
loopback address — the author of a comment is `git config user.name`
as git sees it in the repository, or the system's user name when git has none. Deployed — on any other address — the author is the
value of one request header, `X-Forwarded-Email` unless `--user-header` names
another, set by the auth proxy in front of it; a request that writes a comment
without that header is refused with `401`. Reading needs no header: whoever the
proxy lets through may read.

**Why:** a team that deploys docs already has an identity provider and a proxy
in front of its services. Every login devpage built would be one more to
secure and maintain, and still worse than the one they have. Trusting the
proxy's header keeps devpage free of credentials — at the price that the header
means nothing without the proxy: anyone who can reach devpage directly can set
it.

**Check:** no code stores or checks a password, token or session. A deployed
devpage refuses a comment write that lacks the header, and stamps the header's
value as the author when it is present. The deploying page says devpage must
only be reachable through the proxy.
