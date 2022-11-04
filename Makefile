.PHONY: *

run:
	PUNCHOUT_SERVER_LOGIN_URI=http://example.com/ \
	PUNCHOUT_SERVER_CONFIRMATION_URI=http://localhost:1111/ \
	 cargo run