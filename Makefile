.PHONY: *

run:
	PUNCHOUT_SERVER_LOGIN_URI=http://example.com/?foo=bar \
	PUNCHOUT_SERVER_CONFIRMATION_URI=http://localhost:1111/ \
	OCI_SRM_SERVER_MOCK_BASE_URL=http://localhost:8089/ \
	OCI_SRM_SERVER_MOCK_PORT=8089 \
	 nix run

build:
	nix build .\#docker-image
	IMG_ID=$$(docker load -i result | sed -nr 's/^Loaded image: (.*)$/\1/p' | xargs -I{} docker image ls "{}" --format="{{.ID}}")
	docker tag $$IMG_ID ci-srm-server-mock:latest

docker:
	docker run \
	    -e PUNCHOUT_SERVER_LOGIN_URI=http://example.com/ \
	    -e PUNCHOUT_SERVER_CONFIRMATION_URI=http://localhost:1111/ \
	    -e OCI_SRM_SERVER_MOCK_BASE_URL=http://localhost:8089/ \
	    -e OCI_SRM_SERVER_MOCK_PORT=80 \
	    -p 80:80 \
	    --rm \
	    oci-srm-server-mock \
	    /oci-srm-server-mock