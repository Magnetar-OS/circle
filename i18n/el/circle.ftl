# Ελληνική μετάφραση του Circle.
#
# Σημείωση για τους μεταφραστές: οι πληθυντικοί περνούν από το Fluent, όχι από
# τον κώδικα — το { $count -> } επιλέγει, και οι κανόνες διαφέρουν ανά γλώσσα.
# Μην αντικαταστήσετε έναν επιλογέα με σταθερό κείμενο.

app-title = Circle
app-comment = Βρείτε και επεξεργαστείτε τους ανθρώπους στο ευρετήριό σας
app-keywords = επαφή;επαφές;διεύθυνση;ευρετήριο;vcard;άτομο;άτομα;τηλέφωνο;email;

## Μενού και κέλυφος

file = Αρχείο
edit = Επεξεργασία
view = Προβολή
about = Σχετικά
settings = Ρυθμίσεις
repository = Αποθετήριο
support = Υποστήριξη

all-contacts = Όλες οι επαφές
address-books = Ευρετήρια
contacts-count = { $count } { $count ->
        [one] επαφή
       *[other] επαφές
    }

## Η λίστα

search-contacts = Αναζήτηση επαφών…
no-contacts = Δεν υπάρχουν επαφές ακόμη.
no-contacts-description = Προσθέστε έναν λογαριασμό CardDAV για συγχρονισμό, ή δημιουργήστε μια επαφή.
no-search-results = Καμία επαφή δεν ταιριάζει με «{ $query }».
no-selection = Επιλέξτε μια επαφή για να δείτε τα στοιχεία της.

## Στοιχεία επαφής

email = Email
phone = Τηλέφωνο
address = Διεύθυνση
organisation = Οργανισμός
title = Θέση εργασίας
note = Σημείωση
birthday = Γενέθλια
nickname = Ψευδώνυμο
website = Ιστότοπος
categories = Κατηγορίες
copy = Αντιγραφή
copy-email = Αντιγραφή email
copy-phone = Αντιγραφή τηλεφώνου
copied = Αντιγράφηκε στο πρόχειρο
send-mail = Αποστολή email
call = Κλήση

other-fields = Επίσης σε αυτή την κάρτα
other-fields-description =
    Το Circle τα διατηρεί ακριβώς όπως ήρθαν και δεν τα αλλάζει όταν
    αποθηκεύετε.
has-photo = Φωτογραφία
in-book = Ευρετήριο

## Επεξεργασία

new-contact = Νέα επαφή
edit-contact = Επεξεργασία επαφής
delete-contact = Διαγραφή επαφής
save = Αποθήκευση
cancel = Ακύρωση
delete = Διαγραφή
add = Προσθήκη
remove = Αφαίρεση
refresh = Ανανέωση

name-given = Όνομα
name-family = Επώνυμο
name-additional = Μεσαίο όνομα
name-prefix = Τίτλος
name-suffix = Κατάληξη
display-name = Εμφανιζόμενο όνομα
display-name-description = Αφήστε το κενό για να χρησιμοποιηθεί το όνομα και το επώνυμο.

label-home = Οικία
label-work = Εργασία
label-mobile = Κινητό
label-other = Άλλο

set-preferred = Προτιμώμενο
address-street = Οδός
address-extended = Διαμέρισμα, όροφος
address-locality = Πόλη
address-region = Περιφέρεια
address-postcode = Ταχυδρομικός κώδικας
address-country = Χώρα

birthday-format = Γενέθλια, ως ΕΕΕΕ-ΜΜ-ΗΗ
categories-hint = Χωρίστε τις κατηγορίες με κόμματα. Ένα κόμμα μέσα σε μια κατηγορία γράφεται \\,

add-email = Προσθήκη διεύθυνσης email
add-phone = Προσθήκη τηλεφώνου
add-address = Προσθήκη διεύθυνσης
add-url = Προσθήκη ιστότοπου
add-nickname = Προσθήκη ψευδωνύμου

photo = Φωτογραφία
set-photo = Ορισμός φωτογραφίας…
undo = Αναίρεση
photo-current = Αυτή η κάρτα έχει φωτογραφία.
photo-none = Χωρίς φωτογραφία.
photo-pending = Το { $name } θα οριστεί κατά την αποθήκευση.
photo-removing = Η φωτογραφία αφαιρείται κατά την αποθήκευση.
error-photo = Η φωτογραφία δεν άλλαξε: { $why }

groups = Ομάδες
new-group = Νέα ομάδα
group-name = Όνομα ομάδας
create = Δημιουργία
confirm-delete-group-body =
    Διαγράφεται μόνο η ίδια η ομάδα — τα άτομα σε αυτήν παραμένουν στο
    ευρετήριό σας. Αν το ευρετήριο συγχρονίζεται, η διαγραφή θα σταλεί στον
    διακομιστή στον επόμενο συγχρονισμό.
in-groups = Σε ομάδες

confirm-delete-title = Διαγραφή του { $name };
confirm-delete-body =
    Το αρχείο της επαφής αφαιρείται από αυτό το ευρετήριο. Αν το ευρετήριο
    συγχρονίζεται, η διαγραφή θα σταλεί στον διακομιστή στον επόμενο
    συγχρονισμό.

## Εισαγωγή και εξαγωγή

import = Εισαγωγή επαφών…
export = Εξαγωγή…
import-empty = Τίποτα στο { $path } δεν διαβάστηκε ως επαφή.
import-done = Εισήχθησαν { $added } νέες, ενημερώθηκαν { $updated }.
export-done = Αποθηκεύτηκε το { $path }.
error-remote-file = Αυτή η τοποθεσία δεν είναι τοπικό αρχείο.

## Εισαγωγή CSV

import-csv = Εισαγωγή CSV…
csv-skip = Παράλειψη
csv-uid = Μοναδικό αναγνωριστικό
csv-columns = Αντιστοίχιση { $count } γραμμών
csv-empty = Δεν υπάρχει τίποτα για εισαγωγή σε αυτό το αρχείο.
csv-import-done = Εισήχθησαν { $added }, ενημερώθηκαν { $updated }{ $skipped ->
        [0] {""}
       *[other] , παραλείφθηκαν { $skipped } χωρίς όνομα
    }.

## Ρυθμίσεις

prefer-vcard4 = Οι νέες επαφές χρησιμοποιούν vCard 4.0
prefer-vcard4-description =
    Απενεργοποιημένο γράφει vCard 3.0, που δέχεται κάθε διακομιστής CardDAV.
    Οι υπάρχουσες επαφές διατηρούν πάντα τη δική τους έκδοση.
sort-by-given-name = Ταξινόμηση κατά όνομα
sort-by-given-name-description = Απενεργοποιημένο ταξινομεί κατά επώνυμο, όπως διαβάζεται ένας τηλεφωνικός κατάλογος.
default-book = Οι νέες επαφές πηγαίνουν στο
show-book = Εμφάνιση αυτού του ευρετηρίου

## Σφάλματα

error-load-contacts = Δεν ήταν δυνατή η φόρτωση των επαφών.
error-save = Δεν ήταν δυνατή η αποθήκευση του { $name }: { $why }
error-delete = Δεν ήταν δυνατή η διαγραφή του { $name }: { $why }
error-no-writable-book =
    Δεν υπάρχει εγγράψιμο ευρετήριο. Προσθέστε έναν λογαριασμό CardDAV, ή
    δημιουργήστε έναν κατάλογο μέσα στον φάκελο επαφών σας.
read-only-book = Το { $name } είναι μόνο για ανάγνωση.

## Λογαριασμοί και συγχρονισμός

accounts = Λογαριασμοί
sync = Συγχρονισμός
add-account = Προσθήκη λογαριασμού…
account-name = Όνομα
server-url = Διεύθυνση διακομιστή
username = Όνομα χρήστη
password = Κωδικός πρόσβασης
app-password-hint = Πολλοί πάροχοι απαιτούν κωδικό ειδικό για εφαρμογή αντί για τον κανονικό σας.
no-accounts-description =
    Προσθέστε έναν λογαριασμό CardDAV για να συγχρονίσετε τις επαφές σας με
    έναν διακομιστή. Οι λογαριασμοί είναι κοινοί με το Slate — ένας
    λογαριασμός που προστέθηκε εκεί βρίσκεται ήδη εδώ.
sync-now = Συγχρονισμός τώρα
syncing = Συγχρονισμός…
sync-interval = Αυτόματος συγχρονισμός
sync-interval-description = Πόσο συχνά συγχρονίζονται οι λογαριασμοί μόνοι τους. Ο χειροκίνητος συγχρονισμός λειτουργεί πάντα.
sync-off = Μόνο όταν πατάω Συγχρονισμός
sync-minutes = Κάθε { $minutes ->
        [60] ώρα
       *[other] { $minutes } λεπτά
    }
error-no-account-store = Η αποθήκευση λογαριασμών δεν είναι διαθέσιμη, οπότε δεν μπορούν να αποθηκευτούν λογαριασμοί.
error-url-scheme = Η διεύθυνση του διακομιστή πρέπει να ξεκινά με https://
error-url-insecure = Άρνηση αποστολής του κωδικού σας μέσω μη κρυπτογραφημένης σύνδεσης. Χρησιμοποιήστε https://

## Επιλογή και μαζικές ενέργειες

select = Επιλογή
select-all = Επιλογή όλων
selected-count = { $count ->
        [one] { $count } επιλεγμένη
       *[other] { $count } επιλεγμένες
    }
add-to-group = Προσθήκη σε ομάδα…
add-to-group-title = Προσθήκη { $count ->
        [one] μίας επαφής
       *[other] { $count } επαφών
    } σε ομάδα
add-to-group-body =
    Μπαίνουν στην ομάδα με αυτό το όνομα — ένα υπάρχον όνομα προσθέτει σε
    εκείνη την ομάδα, ένα νέο τη δημιουργεί στην πλαϊνή στήλη.
added-to-group = Προστέθηκαν { $count } στο { $name }.
confirm-delete-many-title = Διαγραφή { $count } επαφών;
deleted-one = Διαγράφηκε το { $name }.
deleted-many = Διαγράφηκαν { $count } επαφές.

## Σύνδεση και διπλότυπα

link = Σύνδεση
unlink = Αποσύνδεση
linked-cards = Συνδεδεμένες κάρτες
link-needs-two = Επιλέξτε τουλάχιστον δύο επαφές για σύνδεση.
linked-count = Συνδέθηκαν { $count } κάρτες σε ένα άτομο.
unlinked = Το { $name } είναι ξανά ξεχωριστή επαφή.
find-duplicates = Εύρεση διπλότυπων…
no-duplicates = Δεν βρέθηκαν πιθανά διπλότυπα.
review-duplicates = Πιθανά διπλότυπα
review-remaining = { $count ->
        [one] Ένα ζεύγος προς έλεγχο
       *[other] { $count } ζεύγη προς έλεγχο
    }
review-explains-linking =
    Η σύνδεση διατηρεί και τις δύο κάρτες ακριβώς όπως είναι και τις εμφανίζει
    ως ένα άτομο. Κάθε κάρτα συνεχίζει να συγχρονίζεται με τον δικό της
    λογαριασμό.
match-email = Και οι δύο έχουν { $value }
match-phone = Και οι δύο έχουν { $value }
match-name = Τα ονόματα μοιάζουν — εικασία, όχι απόδειξη
not-the-same = Δεν είναι το ίδιο άτομο
close = Κλείσιμο
editing-card = Επεξεργασία της κάρτας στο { $book }
share-contact = Κοινοποίηση…
share-hint = Στρέψτε την κάμερα ενός τηλεφώνου στον κώδικα για να προστεθεί η επαφή.
share-too-big = Αυτή η επαφή έχει υπερβολικά πολλά στοιχεία για κώδικα QR.
send = Αποστολή
sms-to = Μήνυμα προς { $number }
sms-via = Στάλθηκε μέσω { $device }.
sms-body = Μήνυμα
sms-sent = Το μήνυμα στάλθηκε.
sms-failed = Το μήνυμα δεν στάλθηκε: { $why }
sms = Μήνυμα
open-link = Άνοιγμα σε πρόγραμμα περιήγησης
back-to-list = Επιστροφή στη λίστα

## Επικοινωνία

keep-in-touch = Επικοινωνία
last-contact = Τελευταία επαφή
last-contacted = { $when }
last-contacted-never = Ποτέ
log-interaction = Καταγραφή επαφής
overdue = Εκπρόθεσμο
notes = Σημειώσεις
note-placeholder = Προσθήκη σημείωσης…
notes-are-local =
    Οι σημειώσεις μένουν σε αυτόν τον υπολογιστή. Δεν γράφονται στην κάρτα της
    επαφής, οπότε τίποτα εδώ δεν στέλνεται σε διακομιστή ούτε το βλέπει
    κάποιος με τον οποίο μοιράζεστε ένα ευρετήριο.

cadence-none = Χωρίς υπενθύμιση
cadence-weekly = Εβδομαδιαία
cadence-fortnightly = Κάθε δεκαπενθήμερο
cadence-monthly = Μηνιαία
cadence-quarterly = Κάθε τρεις μήνες
cadence-twice-yearly = Δύο φορές τον χρόνο
cadence-yearly = Ετήσια

when-today = Σήμερα
when-yesterday = Χθες
when-tomorrow = Αύριο
when-days-ago = { $days ->
        [one] Πριν από { $days } ημέρα
       *[other] Πριν από { $days } ημέρες
    }
when-in-days = { $days ->
        [one] Σε { $days } ημέρα
       *[other] Σε { $days } ημέρες
    }
cadence = Υπενθύμιση επικοινωνίας
related = Σχετικό πρόσωπο
relationships = Σχετικά πρόσωπα
attachment-too-big = Το αρχείο είναι { $size }, πάνω από το όριο των { $limit } για συνημμένα.
attachments = Συνημμένα
attach-file = Επισύναψη αρχείου…
open = Άνοιγμα
attachments-are-local =
    Τα συνημμένα μένουν σε αυτόν τον υπολογιστή, δίπλα στα ευρετήριά σας. Δεν
    γράφονται στην κάρτα της επαφής και δεν φτάνουν ποτέ σε διακομιστή.
attachment-added = Επισυνάφθηκε το { $name }.
attachment-missing = Το { $name } δεν υπάρχει πλέον στον δίσκο.
