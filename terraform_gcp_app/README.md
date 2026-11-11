# GCP App Terraform

This stack manages the Cloud Run service and is applied by `deploy.yml` on every push to `main`. It reads the runtime service account from `../terraform_gcp_storage` via `terraform_remote_state` and fails if that output is missing.

The public invoker binding and the domain mapping live in `../terraform_gcp_edge`, which an administrator applies, because the deployer has no permission to change IAM policies or domain mappings. The `removed` blocks in `main.tf` hand them over without destroying them.

## Usage

Apply `../terraform_gcp_storage` first, then push a container image and add `WEBRTC_GITHUB_CLIENT_SECRET` version `1`.

```bash
cd terraform_gcp_app
terraform plan \
  -var-file=prod.tfvars \
  -var="github_client_id=<github client id>" \
  -var="container_image=asia-northeast1-docker.pkg.dev/webrtc-469410/cloud-run-source-deploy/webrtc@sha256:<digest>"
```
