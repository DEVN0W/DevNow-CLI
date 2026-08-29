from pos_web.app import create_app, run_dev_server


if __name__ == "__main__":
    app = create_app()
    run_dev_server(app)
