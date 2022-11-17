.PHONY: *

run:
	PUNCHOUT_SERVER_LOGIN_URI=http://example.com/?foo=bar \
	PUNCHOUT_SERVER_CONFIRMATION_URI=http://localhost:1111/ \
	 cargo run

buildx:
	docker buildx build -t oci-srm-server-mock --load .

docker:
	docker run \
	    -e PUNCHOUT_SERVER_LOGIN_URI=http://example.com/ \
	    -e PUNCHOUT_SERVER_CONFIRMATION_URI=http://localhost:1111/ \
	    -p 8089:8089 \
	    --rm \
	    oci-srm-server-mock \
	    /oci-srm-server-mock