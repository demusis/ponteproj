use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn data_dir() -> Result<PathBuf> {
    let path = if let Some(path) = std::env::var_os("PONTEPROJ_DATA_DIR") {
        PathBuf::from(path)
    } else {
        directories::BaseDirs::new()
            .context("Perfil local do usuário indisponível")?
            .data_local_dir()
            .join("PonteProj")
    };
    std::fs::create_dir_all(&path).context("Não foi possível criar a pasta local do acervo")?;
    Ok(path)
}

pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("acervo.sqlite3"))
}
