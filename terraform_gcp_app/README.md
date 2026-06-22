# GCP App Terraform

This stack manages the Cloud Run application resources:

- Cloud Run service
- public invoker IAM binding
- Cloud Run domain mapping for `webrtc.duxca.com`

It reads the runtime service account from `../terraform_gcp_storage` via `terraform_remote_state`.

## Usage

Apply `../terraform_gcp_storage` first, then push a container image and add `WEBRTC_GITHUB_CLIENT_SECRET` version `1`.

```bash
cd terraform_gcp_app
terraform plan \
  -var-file=prod.tfvars \
  -var="github_client_id=<github client id>" \
  -var="container_image=asia-northeast1-docker.pkg.dev/webrtc-469410/cloud-run-source-deploy/webrtc@sha256:<digest>"
```
