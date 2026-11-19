//! The Lua scripts the Upstash backend runs (HS2-K9J1RK, `docs/36` §36.17). Redis runs each
//! script atomically, on the Redis clock (`TIME`), so acquire, renew and release are real
//! compare-and-swaps whatever the client's clock says.
//!
//! Every script starts with an identifying comment (`-- hs2rc:<name>:v1`), which a test
//! double reads to know which contract to apply. All replies are arrays of strings, so no
//! integer is rounded and no `nil` truncates an array.
//!
//! Keys of one resource (see `UpstashBackend::keys`):
//! `KEYS[1]` claim hash, `KEYS[2]` fencing counter, `KEYS[3]` the namespace's resource set,
//! `KEYS[4]` the namespace set, `KEYS[5]` the release channel. A script receives only the keys
//! it uses, in this order.

/// How long a lapsed claim stays readable so `renew` can say `expired` rather than
/// `token_invalid`. After it the hash is gone; the fencing counter never is.
pub const GRACE_MS: u64 = 3_600_000;

macro_rules! script {
    ($name:literal, $body:literal) => {
        concat!(
            "-- hs2rc:",
            $name,
            ":v1\n",
            "
local function now_ms()
  local t = redis.call('TIME')
  return tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
end
local function claim_of(key)
  local flat = redis.call('HGETALL', key)
  if #flat == 0 then return nil end
  local c = {}
  for i = 1, #flat, 2 do c[flat[i]] = flat[i + 1] end
  return c
end
",
            $body
        )
    };
}

/// `KEYS`: claim, fencing, resources, namespaces. `ARGV`: claim id, token digest, duration
/// ms, metadata JSON, resource, namespace, grace ms.
/// Replies `held, now, claim_id, fencing, acquired, expires, metadata` or
/// `ok, now, fencing, acquired, expires`.
pub const ACQUIRE: &str = script!(
    "acquire",
    "
local now = now_ms()
local c = claim_of(KEYS[1])
if c and tonumber(c.expires) > now then
  return {'held', tostring(now), c.claim_id, c.fencing, c.acquired, c.expires, c.metadata}
end
local fence = redis.call('INCR', KEYS[2])
local dur = tonumber(ARGV[3])
redis.call('DEL', KEYS[1])
redis.call('HSET', KEYS[1], 'claim_id', ARGV[1], 'digest', ARGV[2], 'fencing', tostring(fence),
  'acquired', tostring(now), 'expires', tostring(now + dur), 'metadata', ARGV[4])
redis.call('PEXPIRE', KEYS[1], dur + tonumber(ARGV[7]))
redis.call('SADD', KEYS[3], ARGV[5])
redis.call('SADD', KEYS[4], ARGV[6])
return {'ok', tostring(now), tostring(fence), tostring(now), tostring(now + dur)}
"
);

/// `KEYS`: claim, channel. `ARGV`: claim id, token digest, duration ms, grace ms.
/// Replies `token_invalid`, `not_holder`, `expired`, or
/// `ok, now, claim_id, fencing, acquired, expires, metadata`.
pub const RENEW: &str = script!(
    "renew",
    "
local now = now_ms()
local c = claim_of(KEYS[1])
if not c then return {'token_invalid'} end
if c.claim_id ~= ARGV[1] then return {'not_holder'} end
if c.digest ~= ARGV[2] then return {'token_invalid'} end
if tonumber(c.expires) <= now then return {'expired'} end
local dur = tonumber(ARGV[3])
local expires = now + dur
local shortened = expires < tonumber(c.expires)
redis.call('HSET', KEYS[1], 'expires', tostring(expires))
redis.call('PEXPIRE', KEYS[1], dur + tonumber(ARGV[4]))
if shortened then redis.call('PUBLISH', KEYS[2], 'shortened') end
return {'ok', tostring(now), c.claim_id, c.fencing, c.acquired, tostring(expires), c.metadata}
"
);

/// `KEYS`: claim, channel. `ARGV`: claim id, token digest. Replies as [`RENEW`]. A lapsed
/// claim is cleared and reported `expired`, like the local backend.
pub const RELEASE: &str = script!(
    "release",
    "
local now = now_ms()
local c = claim_of(KEYS[1])
if not c then return {'token_invalid'} end
if c.claim_id ~= ARGV[1] then return {'not_holder'} end
if c.digest ~= ARGV[2] then return {'token_invalid'} end
redis.call('DEL', KEYS[1])
redis.call('PUBLISH', KEYS[2], 'released')
if tonumber(c.expires) <= now then return {'expired'} end
return {'ok', tostring(now), c.claim_id, c.fencing, c.acquired, c.expires, c.metadata}
"
);

/// `KEYS`: claim. Replies `free, now` or
/// `held, now, claim_id, fencing, acquired, expires, metadata`.
pub const GET: &str = script!(
    "get",
    "
local now = now_ms()
local c = claim_of(KEYS[1])
if c and tonumber(c.expires) > now then
  return {'held', tostring(now), c.claim_id, c.fencing, c.acquired, c.expires, c.metadata}
end
return {'free', tostring(now)}
"
);

/// `KEYS`: claim, fencing. `ARGV`: the fencing token. Replies `ok`, `not_holder`, `expired`
/// or `invalid`.
pub const VERIFY: &str = script!(
    "verify",
    "
local now = now_ms()
local token = tonumber(ARGV[1])
local current = tonumber(redis.call('GET', KEYS[2]) or '0')
if current > token then return {'not_holder'} end
if current < token then return {'invalid'} end
local c = claim_of(KEYS[1])
if c and tonumber(c.fencing) == current and tonumber(c.expires) > now then return {'ok'} end
return {'expired'}
"
);
