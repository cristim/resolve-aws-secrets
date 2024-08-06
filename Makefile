# Makefile for resolve-aws-secrets

# Docker image details
DOCKER_USERNAME ?= $(shell echo $$DOCKER_USERNAME)
IMAGE_NAME := resolve-aws-secrets
TAG := latest

# Check for required environment variables
check_defined = \
    $(strip $(foreach 1,$1, \
        $(call __check_defined,$1,$(strip $(value 2)))))
__check_defined = \
    $(if $(value $1),, \
        $(error Environment variable $1$(if $2, ($2))is not set))

.PHONY: all build-amd64 build-arm64 create-manifest push-manifest clean

all: check-env build-amd64 build-arm64 create-manifest push-manifest

check-env:
	@:$(call check_defined, DOCKER_USERNAME, your Docker Hub username)
	@:$(call check_defined, DOCKER_PASSWORD, your Docker Hub password)
	@echo "$$DOCKER_PASSWORD" | docker login -u "$$DOCKER_USERNAME" --password-stdin

build-amd64:
	docker buildx build --platform linux/amd64 \
		-t $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG)-amd64 \
		--push .

build-arm64:
	docker buildx build --platform linux/arm64 \
		-t $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG)-arm64 \
		--push .

create-manifest:
	docker manifest create $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG) \
		--amend $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG)-amd64 \
		--amend $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG)-arm64

push-manifest:
	docker manifest push $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG)

clean:
	docker rmi $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG)-amd64 || true
	docker rmi $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG)-arm64 || true
	docker manifest rm $(DOCKER_USERNAME)/$(IMAGE_NAME):$(TAG) || true