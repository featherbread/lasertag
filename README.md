# lasertag

`lasertag` takes a list of container images and tells you about new versions:

```
$ lasertag quay.io/prometheus/prometheus:v3.8.0 ghcr.io/syncthing/syncthing:2.0.5
quay.io/prometheus/prometheus:v3.8.0    v3.9.1
ghcr.io/syncthing/syncthing:2.0.5       2.0.13
```

It filters new versions based on the pattern of non-digit characters in the
original tag, automatically selecting semantic versions from repositories that
mix in things like Git SHA tags. This works reasonably well when you run lots
of images with SemVer or date-based tags, and can collect them all from a
configuration file. For example, using [xt](https://github.com/featherbread/xt)
to process an Ansible playbook with `docker_container` tasks, and passing `-d`
to omit images already on the latest version:

```sh
declare -a images
while read -r image; do images+=("$image"); done < <( \
	xt my-ansible-playbook.yml \
	| jq -r '.[].tasks[]["community.docker.docker_container"] // empty | .image' \
	| grep -vE '^localhost/')

lasertag -d "${images[@]}"
```

## Limitations

When passing in an "unstable" tag format, `lasertag`'s automatic matching
effectively locks you into that stream of versions. In this example, you would
never notice a stable `2.11.0` tag after the beta period ends:

```
$ lasertag caddy:2.9.0-beta.1
docker.io/library/caddy:2.9.0-beta.1    2.11.0-beta.2
```

Similarly, `lasertag` won't tell you about patch updates within older major or
minor versions. It always reports the highest available version, regardless of
your readiness and willingness to adopt it.

Finally, `lasertag` is hopeless with identifiers like Git SHAs that don't
follow a stable version-sortable pattern. Basically: if you can't look at a tag
and immediately tell which one is "higher", neither can `lasertag`.
