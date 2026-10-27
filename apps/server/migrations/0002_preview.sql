-- Les aperçus (rendu partiel) ont leur propre fichier, preview.mp4, au lieu d'écraser la vidéo.

alter table artifacts drop constraint artifacts_kind_check;
alter table artifacts add constraint artifacts_kind_check
    check (kind in ('places', 'tour', 'video', 'preview', 'cover', 'publication'));

-- Une vidéo partielle déjà enregistrée n'est pas la vidéo du run : on l'oublie.
delete from artifacts where kind = 'video' and meta ? 'seconds' and meta->>'seconds' is not null;
