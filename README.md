# webrtc.duxca.com

[![Deploy to Main](https://github.com/duxca/webrtc.duxca.com/actions/workflows/deploy.yml/badge.svg?branch=main)](https://github.com/duxca/webrtc.duxca.com/actions/workflows/deploy.yml)

Rust WebRTC signaling server deployed to Cloud Run.

## Configuration

| Variable | Required | Description |
|---|---|---|
| `HOST_ADDR` | yes | Listen address, e.g. `0.0.0.0:8080` |
| `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET` | yes | GitHub OAuth app |
| `REDIRECT_URL` | yes | `https://<host>/oauth/callback`; its origin is also the origin accepted for state-changing requests |
| `ALLOWED_GITHUB_IDS` | no | Comma-separated numeric GitHub user ids allowed to sign in (`gh api users/<login> --jq .id`). Unset or empty: any GitHub account can sign in |

In production `ALLOWED_GITHUB_IDS` comes from the repository variable `WEBRTC_ALLOWED_GITHUB_IDS`.

## Local Development

Register a separate GitHub OAuth app whose callback is `http://127.0.0.1:8080/oauth/callback`, put its values in `.env`, then run `./run_local.bash`. The `local` feature drops the `Secure` flag and `__Host-` prefix from cookies so they work over plain HTTP; it refuses to compile in release builds.

```bash
HOST_ADDR=127.0.0.1:8080
GITHUB_CLIENT_ID=...
GITHUB_CLIENT_SECRET=...
REDIRECT_URL=http://127.0.0.1:8080/oauth/callback
```

## Who Applies What

| Stack | Applied by | Contents |
|---|---|---|
| `terraform_bootstrap/bootstrap` | administrator | registers the state bucket |
| `terraform_gcp_storage` | administrator | APIs, Artifact Registry, runtime service account, secret container |
| `terraform_bootstrap` | administrator | deployer service account, Workload Identity, deployer IAM |
| `terraform_gcp_app` | GitHub Actions (`deploy.yml`) | Cloud Run service |
| `terraform_gcp_edge` | administrator | public invoker binding, domain mapping |
| `terraform_cf` | GitHub Actions (`deploy.yml`) | Cloudflare DNS record |

The deployer can only roll out new revisions of the existing Cloud Run service; anything that changes IAM, APIs or secrets needs an administrator.

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

Set these GitHub repository variables before using the workflows:

- `WEBRTC_GITHUB_CLIENT_ID`
- `WEBRTC_ALLOWED_GITHUB_IDS` (optional, see Configuration)

The deploy job runs in the `production` environment, which the Workload Identity provider requires.

Set this GitHub repository secret if Cloudflare DNS should be managed by GitHub Actions:

- `CLOUDFLARE_API_TOKEN`

### 7. App, Edge And DNS Stacks

Do not run these until an image exists in Artifact Registry. After the first deploy creates the Cloud Run service, an administrator applies `terraform_gcp_edge` to make it public and map the domain:

```bash
cd terraform_gcp_edge
terraform init
terraform plan -var-file=prod.tfvars
terraform apply -var-file=prod.tfvars
cd ..
```

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

