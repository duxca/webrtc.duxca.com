# webrtc.duxca.com

[![Deploy to Main](https://github.com/duxca/webrtc.duxca.com/actions/workflows/deploy.yml/badge.svg?branch=main)](https://github.com/duxca/webrtc.duxca.com/actions/workflows/deploy.yml)

Rust WebRTC signaling server deployed to Cloud Run.

## Initial Infrastructure Setup

Target Google Cloud project:

- project ID: `webrtc-469410`
- project number: `137685276606`
- region: `asia-northeast1`
- domain: `webrtc.duxca.com`

These steps intentionally stop before deploying the app image.

### 1. Select The Project

```bash
gcloud auth login
gcloud config set project webrtc-469410
gcloud config set run/region asia-northeast1
gcloud config set compute/region asia-northeast1
```

### 2. Create The Terraform State Bucket Manually

Bootstrap state also lives in this bucket, so create it before running Terraform.

```bash
gcloud storage buckets create gs://webrtc-469410-terraform-state \
  --project=webrtc-469410 \
  --location=asia-northeast1 \
  --uniform-bucket-level-access

gcloud storage buckets update gs://webrtc-469410-terraform-state \
  --versioning
```

### 3. Register The State Bucket In Terraform

```bash
cd terraform_bootstrap/bootstrap
terraform init
terraform plan -var-file=prod.tfvars
terraform apply -var-file=prod.tfvars
cd ../..
```

### 4. Apply Base GCP Resources

This creates required APIs, Artifact Registry, the Cloud Run runtime service account, and the Secret Manager secret container.

```bash
cd terraform_gcp_storage
terraform init
terraform plan -var-file=prod.tfvars
terraform apply -var-file=prod.tfvars
cd ..
```

### 5. Add Secret Versions

Terraform creates the secret container only. Add the GitHub OAuth client secret manually:

```bash
printf '%s' '<github client secret>' \
  | gcloud secrets versions add WEBRTC_GITHUB_CLIENT_SECRET \
    --project=webrtc-469410 \
    --data-file=-
```

### 6. Apply GitHub Actions Bootstrap

This creates the GitHub Actions deployer service account, Workload Identity Pool/Provider, and IAM bindings.

```bash
cd terraform_bootstrap
terraform init
terraform plan -var-file=prod.tfvars
terraform apply -var-file=prod.tfvars
cd ..
```

Set this GitHub repository variable before using the workflows:

- `WEBRTC_GITHUB_CLIENT_ID`

Set this GitHub repository secret if Cloudflare DNS should be managed by GitHub Actions:

- `CLOUDFLARE_API_TOKEN`

### 7. App And DNS Stacks

Do not run these until an image exists in Artifact Registry.

```bash
cd terraform_gcp_app
terraform init
terraform plan \
  -var-file=prod.tfvars \
  -var='github_client_id=<github client id>' \
  -var='container_image=asia-northeast1-docker.pkg.dev/webrtc-469410/cloud-run-source-deploy/webrtc@sha256:<digest>'
cd ..
```

```bash
cd terraform_cf
CLOUDFLARE_API_TOKEN=... terraform init
CLOUDFLARE_API_TOKEN=... terraform plan -var-file=prod.tfvars
cd ..
```

If the `webrtc.duxca.com` Cloudflare record already exists, import it before applying `terraform_cf`.

## Local Image Push

Build and push manually when needed:

```bash
docker build . --tag asia-northeast1-docker.pkg.dev/webrtc-469410/cloud-run-source-deploy/webrtc:latest
gcloud auth configure-docker asia-northeast1-docker.pkg.dev
docker push asia-northeast1-docker.pkg.dev/webrtc-469410/cloud-run-source-deploy/webrtc:latest
```

Then use either `:latest` or a digest in `terraform_gcp_app`.

