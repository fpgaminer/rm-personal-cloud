# rm-personal-cloud

This is a personal replacement for the reMarkable tablet's cloud servers.

Note that I developed this mostly for my own use, so I am unlikely to accept GitHub issues or pull requests (mostly out of laziness).


## Installation

First, a MITM proxy needs to be installed on the tablet so we can redirect its cloud connection to our personal server.  Note that since our personal cloud server uses SSL the connection remains secure to the tablet.

On device:
* `wget https://github.com/ddvk/rmfakecloud-proxy/releases/download/v0.0.2/installer.sh`
* `chmod +x ./installer.sh`
* `./installer.sh install`

When asked for cloud url enter the URL for the personal cloud server, e.g. "https://rm-personal-cloud.example.com:8084"


## Running

Example execution:

`RUST_BACKTRACE=1 cargo run -- --bind 0.0.0.0 --ssl-cert test.cert --ssl-key test.key --db db.sqlite`


## Development

When tweaking the code it's nice to be able to test it against a real tablet without deploying the code to a production cloud server.

First, you need a DNS entry that points to 127.0.0.1.  For example, imagine you own `localhost.example.com`, point it to 127.0.0.1, and get a valid SSL cert for it.

Point the tablet's proxy at, e.g., `https://localhost.example.com`

Then establish a reverse SSH tunnel: `ssh -R 8084:127.0.0.1:8084 root@reMarkable.local`

Run the development server: `RUST_BACKTRACE=1 cargo run -- --bind 127.0.0.1 --ssl-cert test.cert --ssl-key test.key --db test.sqlite`

Now the tablet will connect securely to the local development server.


## Testing

Run the server: `RUST_BACKTRACE=1 cargo run -- --bind 127.0.0.1 --ssl-cert test.cert --ssl-key test.key --db test.sqlite --hostname localhost.example.com:8084`

Run tests: `python test.py`


## Docker

Example: `docker run -d --init -p 8084:8084 -v /your/ssl/files:/ssl -v /persistant/data:/data rm-personal-cloud`

You'll need a `cert.pem` and `key.pem` under `/your/ssl/`.  rm-personal-cloud will store all its data in `/persistant/data/db.sqlite`.


## New Device Codes

When connecting to the cloud the tablet needs a special authorization code.  `rm-personal-cloud` will print out an admin url on start, which the user can access to generate these authorization codes.  Note that right now the URL that gets printed out is most likely incorrect in production, as it assumes the hostname is the one specified with `--hostname`.  In production that's almost always `local.appspot.com` to make the tablet happy, but obviously that's not a real URL for our server.  You'll need to substitute the server's real hostname to access the admin page.